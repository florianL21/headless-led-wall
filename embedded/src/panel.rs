use crate::CONFIG;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker};
use esp_hal::gpio::{AnyPin, Level, Output, OutputConfig};
use esp_hal::ledc::channel::ChannelIFace;
use esp_hal::ledc::timer::TimerIFace;
use esp_hal::ledc::{timer, LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::peripherals::LCD_CAM;
use esp_hal::time::Rate;
use esp_hub75::framebuffer::{compute_frame_count, compute_rows, latched::DmaFrameBuffer};
use esp_hub75::{Hub75, Hub75Pins8};
use hub75_framebuffer::tiling::{compute_tiled_cols, ChainTopRightDown, TiledFrameBuffer};
use log::{error, info};
use static_cell::make_static;

// Constants to tune for best panel performance
const BITS: u8 = CONFIG.panel.color_depth as u8;
const PANEL_FREQ_WITH_PSRAM: Rate = Rate::from_mhz(2); // Upper limit is about 3Mhz in the best cases when using PSRAM.
const PANEL_FREQ_STATIC: Rate = Rate::from_mhz(20); // caps out at 30Mhz

const TILED_COLS: usize = CONFIG.panel.num_panels_width as usize;
const TILED_ROWS: usize = CONFIG.panel.num_panels_height as usize;
const ROWS: usize = CONFIG.panel.panel_height as usize;
const PANEL_COLS: usize = CONFIG.panel.panel_width as usize;
const FB_COLS: usize = compute_tiled_cols(PANEL_COLS, TILED_ROWS, TILED_COLS);
const NROWS: usize = compute_rows(ROWS);
const FRAME_COUNT: usize = compute_frame_count(BITS);

pub static REFRESH_RATE: AtomicU32 = AtomicU32::new(0);
pub static PANEL_ON: AtomicBool = AtomicBool::new(true);
pub static SYSTEM_IS_UP: AtomicBool = AtomicBool::new(false);
pub static BRIGHTNESS: AtomicU8 = AtomicU8::new(CONFIG.panel.initial_brightness as u8);

type FBType = DmaFrameBuffer<ROWS, FB_COLS, NROWS, BITS, FRAME_COUNT>;
pub type TiledFBType = TiledFrameBuffer<
    FBType,
    ChainTopRightDown<ROWS, PANEL_COLS, TILED_ROWS, TILED_COLS>,
    ROWS,
    PANEL_COLS,
    NROWS,
    BITS,
    FRAME_COUNT,
    TILED_ROWS,
    TILED_COLS,
    FB_COLS,
>;
pub type FrameBufferExchange = Signal<CriticalSectionRawMutex, &'static mut TiledFBType>;

pub struct Hub75Peripherals<'d> {
    pub lcd_cam: LCD_CAM<'d>,
    pub dma_channel: esp_hal::peripherals::DMA_CH0<'d>,
    pub pins: Hub75Pins8<'d>,
    pub pwm_pin: AnyPin<'d>,
    pub ledc: esp_hal::peripherals::LEDC<'d>,
}

fn init_fbs_heap() -> (&'static mut TiledFBType, &'static mut TiledFBType) {
    // If the framebuffer is too large to fit in ram, we can allocate it on the
    // heap in PSRAM instead.
    // Allocate the framebuffer to PSRAM without ever putting it on the stack first
    use alloc::alloc::alloc;
    use alloc::boxed::Box;
    use core::alloc::Layout;

    let layout = Layout::new::<TiledFBType>();

    let fb0 = unsafe {
        let ptr = alloc(layout) as *mut TiledFBType;
        Box::from_raw(ptr)
    };
    let fb1 = unsafe {
        let ptr = alloc(layout) as *mut TiledFBType;
        Box::from_raw(ptr)
    };

    let fb0 = Box::leak(fb0);
    let fb1 = Box::leak(fb1);
    (fb0, fb1)
}

fn init_fbs_stack() -> (&'static mut TiledFBType, &'static mut TiledFBType) {
    // // Allocate the framebuffers in static memory. This assumes that they fit into ram.
    let fb0 = make_static!(TiledFrameBuffer::new());
    let fb1 = make_static!(TiledFrameBuffer::new());
    (fb0, fb1)
}

pub fn init_led_panel<const USE_HEAP: bool>(
) -> (&'static mut TiledFBType, &'static mut TiledFBType, Rate) {
    let (fb0, fb1) = if USE_HEAP {
        init_fbs_heap()
    } else {
        init_fbs_stack()
    };

    let panel_freq = if USE_HEAP {
        PANEL_FREQ_WITH_PSRAM
    } else {
        PANEL_FREQ_STATIC
    };

    (fb0, fb1, panel_freq)
}

#[task]
pub async fn hub75_task(
    peripherals: Hub75Peripherals<'static>,
    rx: &'static FrameBufferExchange,
    tx: &'static FrameBufferExchange,
    fb: &'static mut TiledFBType,
    panel_freq: Rate,
    target_frame_rate: u32,
) {
    info!("hub75_task: starting!");
    let mut brightness = BRIGHTNESS.load(Ordering::Relaxed);

    let (_, tx_descriptors) = esp_hal::dma_descriptors!(0, FBType::dma_buffer_size_bytes());

    let mut hub75 = Hub75::new_async(
        peripherals.lcd_cam,
        peripherals.pins,
        peripherals.dma_channel,
        tx_descriptors,
        panel_freq,
    )
    .expect("failed to create Hub75!");

    let pwm_pin = Output::new(peripherals.pwm_pin, Level::High, OutputConfig::default());

    let mut ledc = Ledc::new(peripherals.ledc);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);

    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(96),
        })
        .expect("Failed to configure LEDC peripheral");
    let mut channel0 = ledc.channel(esp_hal::ledc::channel::Number::Channel0, pwm_pin);
    channel0
        .configure(esp_hal::ledc::channel::config::Config {
            timer: &lstimer0,
            duty_pct: brightness,
            pin_config: esp_hal::ledc::channel::config::PinConfig::PushPull,
        })
        .expect("failed to configure LEDC channel");

    let mut count = 0u32;
    let mut start = Instant::now();

    let mut fb = fb;

    let mut ticker = Ticker::every(Duration::from_millis(
        (1000f32 / target_frame_rate as f32) as u64,
    ));

    let mut panel_is_on = true;
    let mut prev_state: u8 = brightness;

    loop {
        let curr_on_state = PANEL_ON.load(Ordering::Relaxed);
        brightness = BRIGHTNESS.load(Ordering::Relaxed);
        if curr_on_state != panel_is_on {
            if curr_on_state {
                let res = channel0.start_duty_fade(prev_state, brightness, 300);
                info!("Panel fade result: {res:?}");
                prev_state = brightness;
            } else {
                let res = channel0.start_duty_fade(prev_state, 0, 300);
                info!("Panel fade result: {res:?}");
                prev_state = 0;
            }
            panel_is_on = curr_on_state;
        }

        // Render something to the display if:
        // * the rest API requested the panel to be on
        // * there is a brightness transition in progress
        // * the system is having an issue and needs to display an error message on the screen
        if panel_is_on || channel0.is_duty_fade_running() || !SYSTEM_IS_UP.load(Ordering::Relaxed) {
            // Only swap the frame buffer if the display is active.
            // If not there is no need to constantly rerender the UI which
            // should stop automatically if we don't send it a new framebuffer to render to
            if rx.signaled() {
                // if there is a new buffer available, get it and send the old one
                let new_fb = rx.wait().await;
                tx.signal(fb);
                fb = new_fb;
            }

            let mut xfer = hub75
                .render(fb)
                .map_err(|(e, _hub75)| e)
                .expect("failed to start render!");
            if let Err(e) = xfer.wait_for_done().await {
                error!("rendering wait_for_done failed: {e:?}");
            }
            let (result, new_hub75) = xfer.wait();
            hub75 = new_hub75;
            if let Err(e) = result {
                error!("transfer failed: {e:?}");
            }
        }

        ticker.next().await;

        count += 1;
        const FPS_INTERVAL: Duration = Duration::from_secs(1);
        if start.elapsed() > FPS_INTERVAL {
            REFRESH_RATE.store(count, Ordering::Relaxed);
            count = 0;
            start = Instant::now();
        }
    }
}
