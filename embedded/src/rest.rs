use core::sync::atomic::Ordering;

use crate::{
    panel::{BRIGHTNESS, PANEL_ON},
    CONFIG,
};
use alloc::{format, string::String, vec::Vec};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Duration;
use interface::{
    embedded::{CheckedScreenConfig, ScreenBuildError},
    Configuration, Resource,
};
use log::{error, info};
use picoserve::{
    extract::{FromRequest, Query},
    io::Read,
    response::{self, ErrorWithStatusCode},
    routing::{get, post},
    AppBuilder, AppRouter,
};
use postcard::from_bytes;

use crate::flash::{FlashOperation, FlashOperationResult, FLASH_OPERATION, FLASH_OPERATION_RESULT};

pub const WEB_TASK_POOL_SIZE: usize = CONFIG.rest.max_concurrent_connections as usize;

pub type PanelIsOnSignal = Signal<CriticalSectionRawMutex, bool>;
pub type DisplayConfigSignal = Signal<CriticalSectionRawMutex, Option<CheckedScreenConfig>>;

pub static DISPLAY_CONFIG_SIGNAL: DisplayConfigSignal = Signal::new();

pub struct AppProps;

impl AppBuilder for AppProps {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        picoserve::Router::new()
            .route(
                "/",
                get(|| async move {
                    "This is the Public transport display. Please make API requests to /api/.."
                }),
            )
            .route("/api/state", post(on_off_handler))
            .route("/api/config", post(config_handler))
            .route("/api/settings", post(settings_handler))
            .route("/api/storage/format", post(format_handler))
            .route("/api/storage/upload", post(upload_handler))
            .route("/api/storage/exists", post(exists_handler))
            .route("/api/storage/delete", post(delete_handler))
    }
}

pub struct Postcard<T>(pub T);

#[derive(Debug, thiserror::Error, ErrorWithStatusCode)]
#[status_code(BAD_REQUEST)]
pub enum BadPostcardRequest {
    #[error("Read Error")]
    #[status_code(INTERNAL_SERVER_ERROR)]
    ReadError,
    #[error("Postcard deserialize failed: {0}")]
    DeserializationError(#[from] postcard::Error),
}

impl<'r, State, T: serde::Deserialize<'r>> FromRequest<'r, State> for Postcard<T> {
    type Rejection = BadPostcardRequest;

    async fn from_request<R: picoserve::io::Read>(
        _state: &'r State,
        _request_parts: picoserve::request::RequestParts<'r>,
        request_body: picoserve::request::RequestBody<'r, R>,
    ) -> Result<Self, Self::Rejection> {
        Ok(Postcard(from_bytes(
            request_body
                .read_all()
                .await
                .map_err(|_| BadPostcardRequest::ReadError)?,
        )?))
    }
}

pub struct RawData(pub Vec<u8>);

#[derive(Debug, thiserror::Error, ErrorWithStatusCode)]
#[status_code(BAD_REQUEST)]
pub enum BadRawDataRequest {
    #[error("Read Error")]
    #[status_code(INTERNAL_SERVER_ERROR)]
    ReadError,
}

impl<'r, State> FromRequest<'r, State> for RawData {
    type Rejection = BadRawDataRequest;

    async fn from_request<R: picoserve::io::Read>(
        _state: &'r State,
        _request_parts: picoserve::request::RequestParts<'r>,
        request_body: picoserve::request::RequestBody<'r, R>,
    ) -> Result<Self, Self::Rejection> {
        let mut reader = request_body.reader();
        let total_size = reader.content_length();
        let mut data = Vec::with_capacity(total_size);
        loop {
            let mut buf = [0u8; 1024];
            let read_size = reader
                .read(&mut buf)
                .await
                .map_err(|_| BadRawDataRequest::ReadError)?;
            data.extend_from_slice(&buf[..read_size]);
            if read_size == 0 {
                break;
            }
        }

        Ok(RawData(data))
    }
}

#[derive(serde::Deserialize)]
struct PanelStateQuery {
    on: bool,
}

async fn on_off_handler(on: Query<PanelStateQuery>) -> (response::StatusCode, &'static str) {
    PANEL_ON.store(on.0.on, Ordering::Relaxed);
    (response::StatusCode::OK, "State updated")
}

#[derive(serde::Deserialize)]
struct SettingsQuery {
    brightness: u8,
}

async fn settings_handler(settings: Query<SettingsQuery>) -> (response::StatusCode, &'static str) {
    BRIGHTNESS.store(settings.0.brightness, Ordering::Relaxed);
    (response::StatusCode::OK, "Settings updated")
}

async fn format_handler() -> (response::StatusCode, String) {
    DISPLAY_CONFIG_SIGNAL.signal(None);

    FLASH_OPERATION.send(FlashOperation::Format).await;
    match FLASH_OPERATION_RESULT.wait().await {
        Ok(_) => (
            response::StatusCode::OK,
            String::from("Flash formated and config cleared"),
        ),
        Err(e) => (
            response::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to format flash: {e:?}"),
        ),
    }
}

#[derive(serde::Deserialize)]
struct FlashKey {
    key: String,
}

async fn upload_handler(key: Query<FlashKey>, data: RawData) -> (response::StatusCode, String) {
    // info!("Got data: {:?}", data.0);
    let result = postcard::from_bytes::<Resource>(&data.0);
    if let Err(e) = result {
        return (
            response::StatusCode::BAD_REQUEST,
            format!("Failed to deserialize postcard: {e}",),
        );
    }
    FLASH_OPERATION
        .send(FlashOperation::Store(key.0.key, data.0))
        .await;
    match FLASH_OPERATION_RESULT.wait().await {
        Ok(_) => (response::StatusCode::OK, String::from("Item stored")),
        Err(e) => (
            response::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to store item: {e:?}"),
        ),
    }
}

async fn exists_handler(key: Query<FlashKey>) -> (response::StatusCode, String) {
    FLASH_OPERATION
        .send(FlashOperation::Exists(key.0.key))
        .await;
    match FLASH_OPERATION_RESULT.wait().await {
        Err(FlashOperationResult::ExistsResult(exists)) => {
            if exists {
                (response::StatusCode::OK, String::from("Item exists"))
            } else {
                (
                    response::StatusCode::OK,
                    String::from("Item does not exist"),
                )
            }
        }
        other => (
            response::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to check if item exists: {other:?}"),
        ),
    }
}

async fn delete_handler(key: Query<FlashKey>) -> (response::StatusCode, String) {
    FLASH_OPERATION
        .send(FlashOperation::Delete(key.0.key))
        .await;
    match FLASH_OPERATION_RESULT.wait().await {
        Ok(_) => (response::StatusCode::OK, String::from("Item was deleted")),
        Err(e) => {
            error!("Failed to delete item: {e:?}");
            (
                response::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete item: {e:?}"),
            )
        }
    }
}

// TODO: Implement checks that all styles used are also defined
// Check that all used sprites are also in flash
async fn config_handler(
    config: Postcard<Configuration>,
) -> Result<(response::StatusCode, &'static str), ScreenBuildError> {
    info!("Validating config update");
    let config = config.0;
    DISPLAY_CONFIG_SIGNAL.signal(Some(CheckedScreenConfig::new(config)?));
    Ok((response::StatusCode::OK, "Config updated"))
}

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
pub async fn web_task(
    id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<AppProps>,
    config: &'static picoserve::Config<Duration>,
) -> ! {
    let port = 80;
    let mut tcp_rx_buffer = [0; 1024];
    let mut tcp_tx_buffer = [0; 1024];
    let mut http_buffer = [0; 2048];

    picoserve::listen_and_serve(
        id,
        app,
        config,
        stack,
        port,
        &mut tcp_rx_buffer,
        &mut tcp_tx_buffer,
        &mut http_buffer,
    )
    .await
}
