use embassy_net::Runner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_radio::wifi::{Interface, WifiController};
use log::{error, info, warn};

pub enum SystemState {
    WIFIConnecting,
    WIFIWaitForIP,
    WIFIConnected,
    Disconnected,
    Failed,
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
                let info = controller.wait_for_disconnect_async().await.ok();
                warn!("Disconnected: {:?}", info);
                system_state.signal(SystemState::Disconnected);
            }
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                system_state.signal(SystemState::Failed);
            }
        }
        Timer::after(Duration::from_millis(5000)).await
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}
