#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(new_zeroed_alloc)]

use core::ptr::addr_of_mut;
use core::sync::atomic::Ordering;
use embassy_executor::{task, Spawner};
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::gpio::Pin;
use esp_hal::psram::{FlashFreq, PsramConfig, SpiRamFreq, SpiTimingConfigCoreClock};
use esp_hal::system::{CpuControl, Stack};
use esp_hal::timer::AnyTimer;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_hal_embassy::Executor;
use esp_hub75::Hub75Pins8;
use headless_display::flash::{flash_init, flash_task};
use headless_display::panel::init_led_panel;
use headless_display::panel::REFRESH_RATE;
use headless_display::rest::{web_task, AppProps, WEB_TASK_POOL_SIZE};
use headless_display::ui::display_task;
use headless_display::CONFIG;
use headless_display::{
    panel::{hub75_task, FrameBufferExchange, Hub75Peripherals},
    wifi::{connection, net_task, CurrentStateSignal, SystemState},
};
use log::info;
use picoserve::AppBuilder;
use static_cell::make_static;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

const TARGET_PANEL_FRAME_RATE: u32 = CONFIG.panel.target_fps as u32;

#[task]
async fn log_fps() {
    loop {
        Timer::after(Duration::from_millis(1000)).await;
        info!("FPS: {}", REFRESH_RATE.load(Ordering::Relaxed));
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let psram_config = PsramConfig {
        flash_frequency: FlashFreq::FlashFreq120m,
        ram_frequency: SpiRamFreq::Freq120m,
        core_clock: SpiTimingConfigCoreClock::SpiTimingConfigCoreClock240m,
        ..Default::default()
    };
    let config = esp_hal::Config::default()
        .with_cpu_clock(CpuClock::max())
        .with_psram(psram_config);
    let peripherals = esp_hal::init(config);

    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timer0: AnyTimer = timg0.timer0.into();
    let timer1: AnyTimer = timg0.timer1.into();
    esp_hal_embassy::init([timer0, timer1]);
    let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);

    info!("Embassy initialized!");

    static CURRENT_STATE: CurrentStateSignal = CurrentStateSignal::new();

    // Initialize flash storage
    let flash = flash_init();
    let flash = make_static!(flash);
    let flash = &*flash;

    // LED Panel init
    let pins = Hub75Pins8 {
        red1: peripherals.GPIO42.degrade(),
        grn1: peripherals.GPIO41.degrade(),
        blu1: peripherals.GPIO40.degrade(),
        red2: peripherals.GPIO38.degrade(),
        grn2: peripherals.GPIO39.degrade(),
        blu2: peripherals.GPIO12.degrade(),
        clock: peripherals.GPIO2.degrade(),
        blank: peripherals.GPIO14.degrade(),
        latch: peripherals.GPIO47.degrade(),
    };

    let hub75_per: Hub75Peripherals<'_> = Hub75Peripherals {
        dma_channel: peripherals.DMA_CH0,
        lcd_cam: peripherals.LCD_CAM,
        pins,
        pwm_pin: peripherals.GPIO45.degrade(),
        ledc: peripherals.LEDC,
    };
    let (fb0, fb1, panel_freq) = init_led_panel::<false>();

    info!("init framebuffer exchange");
    static TX: FrameBufferExchange = FrameBufferExchange::new();
    static RX: FrameBufferExchange = FrameBufferExchange::new();

    static mut APP_CORE_STACK: Stack<4096> = Stack::new();
    let _guard = cpu_control
        .start_app_core(unsafe { &mut *addr_of_mut!(APP_CORE_STACK) }, move || {
            info!("Core 1 spawning all tasks");

            let lp_executor = make_static!(Executor::new());
            // display task runs as low priority task
            lp_executor.run(|spawner| {
                spawner
                    .spawn(hub75_task(
                        hub75_per,
                        &RX,
                        &TX,
                        fb1,
                        panel_freq,
                        TARGET_PANEL_FRAME_RATE,
                    ))
                    .ok();
            });
        })
        .unwrap();

    spawner.must_spawn(flash_task(flash, cpu_control));
    spawner.must_spawn(display_task(&TX, &RX, fb0, &CURRENT_STATE, flash));

    let stats = esp_alloc::HEAP.stats();
    info!("After panel alloc: {stats}");

    // spawner.must_spawn(log_fps());

    // // WIFI init
    // // Allocate the WIFI stack to the internal heap
    esp_alloc::heap_allocator!(size: 72 * 1024);
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG1);
    let esp_wifi_ctrl = &*make_static!(
        esp_wifi::init(timer1.timer0, rng).expect("Failed to initialize WIFI/BLE controller")
    );
    let (controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI)
        .expect("Failed to initialize WIFI controller");

    let wifi_interface = interfaces.sta;
    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        make_static!(StackResources::<3>::new()),
        seed,
    );

    spawner.must_spawn(connection(controller, &CURRENT_STATE));
    spawner.must_spawn(net_task(runner));

    let stats = esp_alloc::HEAP.stats();
    info!("Total used heap: {stats}");

    // TODO: handle system start properly. The wifi logo flashes briefly because the system is set to ready from 2 locations
    CURRENT_STATE.signal(SystemState::WIFIConnecting);
    loop {
        if stack.is_link_up() {
            CURRENT_STATE.signal(SystemState::WIFIWaitForIP);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    info!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }
    CURRENT_STATE.signal(SystemState::Ready);

    // Webserver

    let app = make_static!(AppProps.build_app());

    let config = make_static!(picoserve::Config::new(picoserve::Timeouts {
        start_read_request: Some(Duration::from_secs(5)),
        persistent_start_read_request: Some(Duration::from_secs(1)),
        read_request: Some(Duration::from_secs(1)),
        write: Some(Duration::from_secs(1)),
    })
    .keep_connection_alive());

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(id, stack, app, config));
    }

    loop {
        Timer::after(Duration::from_secs(20)).await;
    }
}
