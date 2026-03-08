use embassy_time::{Duration, Instant};
use embedded_io_async::Read;
use picoserve::response::IntoResponse;

pub struct OTARequest;

impl picoserve::routing::RequestHandlerService<()> for OTARequest {
    async fn call_request_handler_service<
        R: Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        (): &(),
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        if request.body_connection.content_length() > 2_000_000 {
            let response = (
                picoserve::response::StatusCode::PAYLOAD_TOO_LARGE,
                "The file must be smaller than 2MB",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;

            log::info!("Too large");

            return response;
        }

        let timeout = Duration::from_nanos(request.body_connection.content_length() as u64 * 100);

        log::info!("Allowed time: {:.2}s", timeout.as_millis() as f32 / 1000.0);
        let start_time = Instant::now();

        let mut reader = request
            .body_connection
            .body()
            .reader()
            // If you use the embassy feature, using `with_different_timeout` is preferable.
            .with_different_timeout(Duration::from_secs(120));

        let mut buffer = [0; 1024];

        let mut upload_byte_count = 0_usize;

        let mut last_log_time = Instant::now();

        loop {
            let read_size = reader.read(&mut buffer).await?;
            if read_size == 0 {
                break;
            }

            upload_byte_count += read_size;

            //TODO: flash the actual OTA update data

            if last_log_time.elapsed() > Duration::from_secs(1) {
                last_log_time = Instant::now();

                log::info!(
                    "Upload progress: {:.2}%",
                    100.0 * (upload_byte_count as f32) / (reader.content_length() as f32),
                )
            }
        }

        log::info!(
            "Done in {:.2}s",
            start_time.elapsed().as_millis() as f32 / 1000.0,
        );

        format_args!("Upload finished\r\n")
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await
    }
}
