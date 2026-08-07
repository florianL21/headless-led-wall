#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::gpio::Pin;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::ram;
use esp_hal::rng::Rng;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use esp_hub75::Hub75Pins8;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{self, ControllerConfig};
use headless_display::CONFIG;
use headless_display::flash::{FlashType, flash_init, flash_task};
use headless_display::panel::init_led_panel;
use headless_display::rest::{AppProps, WEB_TASK_POOL_SIZE, web_task};
use headless_display::wifi::CURRENT_STATE;
use headless_display::{
    panel::{Hub75Peripherals, hub75_task},
    wifi::{SystemState, connection, net_task},
};
use log::info;
use picoserve::{AppBuilder, AppRouter};
use static_cell::StaticCell;

#[cfg(feature = "esp32s3")]
use esp_hal::system::Stack;

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

const TARGET_PANEL_FRAME_RATE: u32 = CONFIG.panel.target_fps as u32;
const SSID: &str = CONFIG.wifi.ssid;
const PASSWORD: &str = CONFIG.wifi.password;

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());

    esp_alloc::heap_allocator!(size: 82 * 1024);
    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64000);

    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    #[cfg(feature = "psram")]
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    info!("Embassy initialized!");

    // Initialize flash storage
    let flash = flash_init(peripherals.FLASH);
    static FLASH: StaticCell<FlashType> = StaticCell::new();
    let flash = FLASH.init(flash);
    let flash = &*flash;

    // LED Panel init
    let (pins, pwm_pin) = headless_display::gpio_pins!(peripherals);

    let hub75_per: Hub75Peripherals<'_> = Hub75Peripherals {
        dma_channel: peripherals.DMA_CH0,
        #[cfg(feature = "esp32c6")]
        interface: peripherals.PARL_IO,
        #[cfg(feature = "esp32s3")]
        interface: peripherals.LCD_CAM,
        pins,
        pwm_pin,
        ledc: peripherals.LEDC,
    };
    let (fbs, panel_freq) = init_led_panel::<false>();

    cfg_if::cfg_if! {
        if #[cfg(feature = "esp32c6")] {
            use esp_rtos::embassy::InterruptExecutor;
            use esp_hal::interrupt::Priority;

            static EXECUTOR: StaticCell<InterruptExecutor<2>> = StaticCell::new();
            let executor = InterruptExecutor::new(sw_int.software_interrupt2);
            let executor = EXECUTOR.init(executor);
            let high_prio_spawner = executor.start(Priority::max());
            high_prio_spawner.spawn(hub75_task(
                hub75_per,
                fbs,
                panel_freq,
                TARGET_PANEL_FRAME_RATE,
                flash
            ).unwrap());
        } else if #[cfg(feature = "esp32s3")] {
            use esp_rtos::embassy::Executor;

            static APP_CORE_STACK: StaticCell<Stack<16384>> = StaticCell::new();
            let app_core_stack = APP_CORE_STACK.init(Stack::new());
            esp_rtos::start_second_core(
                peripherals.CPU_CTRL,
                sw_int.software_interrupt1,
                app_core_stack,
                move || {
                    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
                    let executor = EXECUTOR.init(Executor::new());
                    executor.run(|spawner| {
                        spawner.spawn(hub75_task(
                        hub75_per,
                        fbs,
                        panel_freq,
                        TARGET_PANEL_FRAME_RATE,
                        flash
                    ).unwrap());
                    });
                },
            );
        }
    }

    spawner.spawn(flash_task(flash).unwrap());

    let stats = esp_alloc::HEAP.stats();
    info!("After panel alloc: {stats}");

    // WIFI init
    // Allocate the WIFI stack to the internal heap

    let station_config = wifi::Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into()),
    );

    info!("Starting wifi");
    let wifi_interface = esp_radio::wifi::Interface::station();
    let controller = esp_radio::wifi::WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();
    info!("Wifi configured and started!");

    // let wifi_interface = interfaces.station;

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

    spawner.spawn(connection(controller, &CURRENT_STATE).unwrap());
    spawner.spawn(net_task(runner).unwrap());

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
        spawner.spawn(web_task(id, stack, app).unwrap());
    }

    // loop {
    //     let stats = esp_alloc::HEAP.stats();
    //     info!("Total used heap: {stats}");
    //     Timer::after_secs(10).await;
    // }
}
