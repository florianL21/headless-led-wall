use anyhow::Result;
use log::{error, info};
use std::net::Ipv4Addr;
use tokio::select;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;

use crate::config::ServerConfig;
use crate::display::build_display;
use crate::weather::{WeatherData, WeatherUpdateResult, get_weather_data};
use crate::wl::{TransportData, get_transport_data};

const WL_POLL_RATE: Duration = Duration::from_secs(45);
pub const WEATHER_POLL_RATE: Duration = Duration::from_secs(60 * 10); // 10 minutes
const TIME_POLL_RATE: Duration = Duration::from_secs(30);
const RETRY_POLL_RATE: Duration = Duration::from_secs(5);

pub enum DataUpdate {
    Transport(TransportData),
    Weather(WeatherData),
    Ping,
}

/// Periodically fetch transport data and send it to the display update task
pub async fn fetch_transport_data(
    token: CancellationToken,
    tx: Sender<DataUpdate>,
    client: &reqwest::Client,
    config: ServerConfig,
) -> Result<()> {
    let mut interval = time::interval(WL_POLL_RATE);
    let mut last_data = None;
    let station_query = config.build_wl_query();
    loop {
        select! {
            _ = interval.tick() => {
            }
            _ = token.cancelled() => {
                return Ok(());
            }
        }
        match get_transport_data(client, &station_query, &config.line_filter).await {
            Err(e) => {
                error!("Failed to fetch transport data: {e}");
            }
            Ok(data) => {
                if let Some(ref last_data) = last_data
                    && last_data == &data
                {
                    info!("Transport data has not changed. Skipping display update");
                    continue;
                }
                if let Err(e) = tx.send(DataUpdate::Transport(data.clone())).await {
                    // Assume shutdown of the server.
                    info!("Channel closed ({e}), stopping fetch_transport_data task.");
                    return Ok(());
                }
                last_data = Some(data);
            }
        }
    }
}

/// Periodically fetch weather data and send it to the display update task
pub async fn fetch_weather_data(
    token: CancellationToken,
    tx: Sender<DataUpdate>,
    client: &reqwest::Client,
    config: ServerConfig,
) -> Result<()> {
    let mut interval = time::interval(WEATHER_POLL_RATE);
    let mut last_update = None;
    let mut last_data = None;
    loop {
        select! {
            _ = interval.tick() => {
            }
            _ = token.cancelled() => {
                return Ok(());
            }
        }
        match get_weather_data(&mut last_update, client, &config.met).await {
            Err(e) => {
                error!("Failed to fetch weather data: {e}");
            }
            Ok(WeatherUpdateResult::Updated(data, next_check)) => {
                last_data = Some(data.clone());
                interval = time::interval(next_check);
                // First tick completes immediately
                interval.tick().await;
                info!("next weather fetch: {next_check:#?}");
                if let Err(e) = tx.send(DataUpdate::Weather(data)).await {
                    // Assume shutdown of the server.
                    info!("Channel closed ({e}), stopping fetch_weather_data task.");
                    return Ok(());
                }
            }
            Ok(WeatherUpdateResult::Unchanged(next_check)) => {
                interval = time::interval(next_check);
                // First tick completes immediately
                interval.tick().await;
                info!("Unchanged, next weather fetch: {next_check:#?}");
                if let Some(ref data) = last_data
                    && let Err(e) = tx.send(DataUpdate::Weather(data.clone())).await
                {
                    // Assume shutdown of the server.
                    info!("Channel closed ({e}), stopping fetch_weather_data task.");
                    return Ok(());
                }
            }
        }
    }
}

/// Task to keep the display up to date.
/// For example if a previous push to the display failed because it could not be reached
/// trigger a ping command to push to the display faster than usual.
/// Also ping about every 30s just to keep the time up to date.
pub async fn maintain_display(token: CancellationToken, tx: Sender<DataUpdate>) -> Result<()> {
    let mut time_interval = time::interval(TIME_POLL_RATE);

    loop {
        select! {
            _ = time_interval.tick() => {
                if let Err(e) = tx.send(DataUpdate::Ping).await {
                    // Assume shutdown of the server
                    info!("Channel closed ({e}), stopping maintain_display task.");
                    return Ok(());
                }
            }
            _ = token.cancelled() => {
                return Ok(());
            }
        }
    }
}

pub async fn push_display_update(
    token: CancellationToken,
    ip: Ipv4Addr,
    mut rx: Receiver<DataUpdate>,
) -> Result<()> {
    let mut current_weather = None;
    let mut current_transport = None;
    let mut last_send_failed = false;
    let mut retry_ticker = time::interval(RETRY_POLL_RATE);
    let client = reqwest::Client::new();
    loop {
        select! {
            data = rx.recv() => {
                let data = if let Some(data) = data {
                    data
                } else {
                    info!("Channel closed");
                    return Ok(()); // Exit if the channel is closed
                };
                match data {
                    DataUpdate::Weather(data) => {
                        current_weather = Some(data);
                    }
                    DataUpdate::Transport(data) => {
                        current_transport = Some(data);
                    }
                    DataUpdate::Ping => {
                        // This is here to trigger a screen refresh
                    }
                }
            }
            _ = retry_ticker.tick() => {
                // If the last send succeeded ignore this ticker
                if !last_send_failed  {
                    continue;
                }
            }
            _ = token.cancelled() => {
                return Ok(());
            }
        };

        if let Some(current_weather) = &current_weather
            && let Some(current_transport) = &current_transport
        {
            let display_data = build_display(current_weather, current_transport);
            let buf = postcard::to_allocvec(&display_data);
            let buf = match buf {
                Ok(buf) => buf,
                Err(e) => {
                    error!("Failed to serialize display data: {e}");
                    continue;
                }
            };
            let res = client
                .post(format!("http://{ip}/api/config"))
                .body(buf)
                .timeout(Duration::from_secs(3))
                .send()
                .await;
            let resp = match res {
                Ok(resp) => resp,
                Err(e) => {
                    error!("Failed to send display data: {e}");
                    last_send_failed = true;
                    continue;
                }
            };
            if !resp.status().is_success() {
                error!("Display responded with error: {:?}", resp.text().await);
                last_send_failed = true;
                continue;
            }
        }

        last_send_failed = false;
    }
}
