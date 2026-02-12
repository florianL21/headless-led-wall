#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use core::sync::atomic::Ordering;
use embassy_executor::{task, Spawner};
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::gpio::Pin;
use esp_hal::interrupt::software::SoftwareInterruptControl;
// use esp_hal::psram::{FlashFreq, PsramConfig, SpiRamFreq, SpiTimingConfigCoreClock};
use esp_hal::rng::Rng;
// use esp_hal::system::{CpuControl, Stack};
use esp_hal::timer::AnyTimer;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_hub75::Hub75Pins8;
use esp_rtos::embassy::Executor;
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
use picoserve::{AppBuilder, AppRouter};
use static_cell::{make_static, StaticCell};

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

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    esp_alloc::heap_allocator!(size: 72 * 1024);
    // let psram_config = PsramConfig {
    //     flash_frequency: FlashFreq::FlashFreq120m,
    //     ram_frequency: SpiRamFreq::Freq120m,
    //     core_clock: Some(SpiTimingConfigCoreClock::SpiTimingConfigCoreClock240m),
    //     ..Default::default()
    // };
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    // .with_psram(psram_config);
    let peripherals = esp_hal::init(config);

    // esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    // let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);

    info!("Embassy initialized!");

    static CURRENT_STATE: CurrentStateSignal = CurrentStateSignal::new();

    // Initialize flash storage
    let flash = flash_init(peripherals.FLASH);
    // static FLASH: StaticCell<> = StaticCell::new();
    let flash = make_static!(flash);
    let flash = &*flash;

    // LED Panel init
    let pins = Hub75Pins8 {
        red1: peripherals.GPIO6.degrade(),
        grn1: peripherals.GPIO7.degrade(),
        blu1: peripherals.GPIO0.degrade(),
        red2: peripherals.GPIO1.degrade(),
        grn2: peripherals.GPIO4.degrade(),
        blu2: peripherals.GPIO10.degrade(),
        clock: peripherals.GPIO11.degrade(),
        blank: peripherals.GPIO2.degrade(),
        latch: peripherals.GPIO5.degrade(),
    };

    let hub75_per: Hub75Peripherals<'_> = Hub75Peripherals {
        dma_channel: peripherals.DMA_CH0,
        interface: peripherals.PARL_IO,
        pins,
        pwm_pin: peripherals.GPIO3.degrade(),
        ledc: peripherals.LEDC,
    };
    let (fb0, fb1, panel_freq) = init_led_panel::<false>();

    info!("init framebuffer exchange");
    static TX: FrameBufferExchange = FrameBufferExchange::new();
    static RX: FrameBufferExchange = FrameBufferExchange::new();

    // static APP_CORE_STACK: StaticCell<Stack<8192>> = StaticCell::new();
    // let app_core_stack = APP_CORE_STACK.init(Stack::new());
    // esp_rtos::start_second_core(
    //     peripherals.CPU_CTRL,
    //     sw_int.software_interrupt0,
    //     sw_int.software_interrupt1,
    //     app_core_stack,
    //     move || {
    //         static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    //         let executor = EXECUTOR.init(Executor::new());
    //         executor.run(|spawner| {});
    //     },
    // );
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

    spawner.must_spawn(flash_task(flash));
    spawner.must_spawn(display_task(&TX, &RX, fb0, &CURRENT_STATE, flash));

    let stats = esp_alloc::HEAP.stats();
    info!("After panel alloc: {stats}");

    // spawner.must_spawn(log_fps());

    // // WIFI init
    // // Allocate the WIFI stack to the internal heap

    static RADIO_INIT: StaticCell<esp_radio::Controller> = StaticCell::new();
    let radio_init =
        RADIO_INIT.init(esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller"));
    let (controller, interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    static STACK_RES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        STACK_RES.init(StackResources::new()),
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
    static APP: StaticCell<AppRouter<AppProps>> = StaticCell::new();
    let app = APP.init(AppProps.build_app());

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(id, stack, app));
    }

    loop {
        Timer::after(Duration::from_secs(20)).await;
    }
}
