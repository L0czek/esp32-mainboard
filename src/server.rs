use defmt::info;
use embassy_time::Duration;
use picoserve::{
    make_static,
    response::{File, IntoResponse},
    routing::{self, get, parse_path_segment, PathRouter},
    AppBuilder, AppRouter, Router,
};
use serde::Serialize;

use crate::html::INDEX_HTML;
use crate::wifi::WifiResources;

// Define the pool size for web tasks
const WEB_TASK_POOL_SIZE: usize = 8;

#[derive(Serialize)]
struct APIResponse {
    ok: bool,
}

// App properties for the web server
struct AppProps;

impl AppBuilder for AppProps {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        Router::new()
            .route("/", routing::get_service(File::html(INDEX_HTML)))
            .route(
                (
                    "/api/digital",
                    parse_path_segment::<u8>(),
                    parse_path_segment::<u8>(),
                ),
                get(digital_handler),
            )
    }
}

// Handler for digital I/O API
async fn digital_handler((id, value): (u8, u8)) -> impl IntoResponse {
    info!("Setting digital pin {} to {}", id, value);

    // Here you'd implement actual pin control
    // For now, just return a success message
    picoserve::response::Json(APIResponse { ok: true })
}

/// Initialize and run the web server
///
/// This function sets up the picoserve server using the provided WiFi resources
/// and spawns tasks to handle web requests.
pub async fn run_server(spawner: embassy_executor::Spawner, wifi_resources: WifiResources) {
    let WifiResources {
        ap_stack,
        sta_stack,
    } = wifi_resources;

    // Create the router app
    let app = make_static!(AppRouter<AppProps>, AppProps.build_app());

    // Configure server timeouts
    let config = make_static!(
        picoserve::Config<Duration>,
        picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(1)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(1)),
        })
        .keep_connection_alive()
    );

    // No need for buffer allocation here

    // Start web tasks for AP interface
    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.spawn(web_task(id, ap_stack, app, config)).unwrap();
    }

    // Start web tasks for STA interface
    for id in 0..WEB_TASK_POOL_SIZE {
        spawner
            .spawn(web_task(id + WEB_TASK_POOL_SIZE, sta_stack, app, config))
            .unwrap();
    }

    info!(
        "Web server started on both AP ({}:80) and STA interfaces ({}:80)",
        ap_stack.config_v4().map(|c| c.address),
        sta_stack.config_v4().map(|c| c.address)
    );
}

// Web task function that handles HTTP requests
#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE * 2)]
async fn web_task(
    id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<AppProps>,
    config: &'static picoserve::Config<Duration>,
) -> ! {
    let port = 80;

    // Allocate buffers inside the task
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
