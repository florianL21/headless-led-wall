use crate::CONFIG;
use embassy_net::Runner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};
use log::{error, info};

const SSID: &str = CONFIG.wifi.ssid;
const PASSWORD: &str = CONFIG.wifi.password;

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
    info!("Device capabilities: {:?}", controller.capabilities());
    loop {
        if esp_wifi::wifi::wifi_state() == WifiState::StaConnected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            system_state.signal(SystemState::Disconnected);
            Timer::after(Duration::from_millis(5000)).await
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.into(),
                password: PASSWORD.into(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("Starting wifi");
            controller.start_async().await.unwrap();
            info!("Wifi started!");
        }
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => {
                info!("Wifi connected!");
                system_state.signal(SystemState::WIFIConnected);
            }
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                system_state.signal(SystemState::Failed);
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
