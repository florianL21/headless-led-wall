use embassy_futures::select::{Either, select};
use embassy_net::Runner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{Interface, WifiController, WifiError};
use log::{error, info, warn};

use crate::{CONFIG, rest::LAST_REST_UPDATE};

const UPDATE_TIMEOUT: u64 = CONFIG.rest.update_watchdog_timeout as u64;
pub static CURRENT_STATE: CurrentStateSignal = CurrentStateSignal::new();
static RECONNECT_TRIGGER: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub enum SystemState {
    WIFIConnecting,
    WIFIWaitForIP,
    WIFIConnected,
    Disconnected,
    Failed(WifiError),
    Ready,
}

pub type CurrentStateSignal = Signal<CriticalSectionRawMutex, SystemState>;

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    system_state: &'static CurrentStateSignal,
) {
    info!("start connection task");

    loop {
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(info) => {
                info!("Wifi connected to {:?}", info);
                system_state.signal(SystemState::WIFIConnected);
                // wait until we're no longer connected
                match select(
                    RECONNECT_TRIGGER.wait(),
                    controller.wait_for_disconnect_async(),
                )
                .await
                {
                    Either::First(_) => {
                        let info = controller.disconnect_async().await;
                        warn!("Forced reconnect: {:?}", info);
                    }
                    Either::Second(info) => {
                        warn!("Disconnected: {:?}", info);
                        system_state.signal(SystemState::Disconnected);
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                system_state.signal(SystemState::Failed(e));
            }
        }
        Timer::after(Duration::from_millis(5000)).await
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}

#[embassy_executor::task]
pub async fn connection_watchdog() {
    loop {
        let updated = LAST_REST_UPDATE.wait();
        let timeout = Timer::after(Duration::from_secs(UPDATE_TIMEOUT));
        match select(updated, timeout).await {
            Either::First(_) => {}
            Either::Second(_) => RECONNECT_TRIGGER.signal(()),
        }
    }
}
