#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use esp_hal::timer::systimer::SystemTimer;
use esp_wifi::EspWifiController;
use rand_core::RngCore;
use spin::RwLock;

use alloc::string::{String, ToString};
use mainboard::{create_board, html::INDEX_HTML, Board};

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::{Config, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::wifi::{ClientConfiguration, Configuration};
use once_cell::sync::OnceCell;
use panic_rtt_target as _;
use picoserve::response::{ErrorWithStatusCode, File, IntoResponse, Response};
use picoserve::routing::{self, parse_path_segment};
use picoserve::{make_static, AppBuilder, AppRouter};
use picoserve::{routing::get, Router};
use static_cell::StaticCell;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

// Network configuration
static WIFI_SSID: &str = "internet.www_2.4GHz";
static WIFI_PASSWORD: &str = "warsztatywww.pl";
// static AP_SSID: &str = "ESP32-AP";
// static AP_PASSWORD: &str = "password123";

// Allocate network resources
static ESP_WIFI_CONTROLLER: StaticCell<EspWifiController> = StaticCell::new();
static STACK: StaticCell<Stack<'static>> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();

// App state to share with handlers
#[derive(Debug)]
struct AppState {
    d0: Output<'static>,
    d1: Output<'static>,
}

static APP_STATE: OnceCell<RwLock<AppState>> = OnceCell::new();

pub struct Application;

impl AppBuilder for Application {
    type PathRouter = impl routing::PathRouter;

    fn build_app(self) -> Router<Self::PathRouter> {
        picoserve::Router::new()
            .route("/", routing::get_service(File::html(INDEX_HTML)))
            .route(
                (
                    "/api/digital",
                    parse_path_segment::<u8>(),
                    parse_path_segment::<u8>(),
                ),
                get(digital_control),
            )
    }
}

#[derive(Debug, thiserror::Error, ErrorWithStatusCode)]
#[status_code(BAD_REQUEST)]
enum BadRequest {
    #[error("Bad: {0}")]
    Bad(String),
}

// Handler for digital pin control
async fn digital_control((pin, value): (u8, u8)) -> impl IntoResponse {
    let pin: u8 = match pin {
        0 | 1 => pin,
        _ => return Err(BadRequest::Bad("Invalid pin".to_string())),
    };

    let value: u8 = match value {
        0 | 1 => value,
        _ => return Err(BadRequest::Bad("Invalid value".to_string())),
    };

    let mut state = APP_STATE.get().unwrap().write();

    match (pin, value) {
        (0, 0) => state.d0.set_low(),
        (0, 1) => state.d0.set_high(),
        (1, 0) => state.d1.set_low(),
        (1, 1) => state.d1.set_high(),
        _ => unreachable!(),
    }

    Ok(Response::ok("{\"status\":\"ok\"}"))
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // Initialize RTT for debugging
    rtt_target::rtt_init_defmt!();

    // Configure ESP32 with maximum CPU clock
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    // Create a board with the peripherals
    let board = create_board!(peripherals);
    initialize_appstate(board);

    // Create stuff for WiFi
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    // Initialize WiFi
    let esp_wifi_controller = ESP_WIFI_CONTROLLER
        .init_with(|| esp_wifi::init(timer1.timer0, rng).expect("Failed to initialize WiFi"));

    // Create WiFi device
    let (mut wifi_controller, interfaces) =
        esp_wifi::wifi::new(esp_wifi_controller, peripherals.WIFI)
            .expect("Failed to create WiFi device");

    // Extract the WiFi device from interfaces
    let wifi_device = interfaces.sta;

    // Configure WiFi client
    let client_config = Configuration::Client(ClientConfiguration {
        ssid: WIFI_SSID.into(),
        password: WIFI_PASSWORD.into(),
        ..Default::default()
    });

    // Start WiFi
    wifi_controller
        .set_configuration(&client_config)
        .expect("Failed to configure WiFi");

    wifi_controller.start().expect("Failed to start WiFi");

    info!("WiFi started");

    // Wait for WiFi connection
    info!("Waiting for WiFi connection");

    wifi_controller.connect().expect("wifi no worky");

    // TODO: Add fallback to AP mode if connection fails

    // Initialize network stack
    let config = Config::dhcpv4(Default::default());

    // Create the stack resources
    let resources = RESOURCES.init(StackResources::new());

    // Create the stack and runner
    let (stack, runner) = embassy_net::new(wifi_device, config, resources, rng.next_u64());

    // Initialize the static stack
    let stack = STACK.init(stack);

    info!("Network stack initialized");

    // Spawn the network runner task
    spawner.spawn(net_runner_task(runner)).ok();

    // TODO: Write AI-like description of this
    info!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let app = make_static!(AppRouter<Application>, Application {}.build_app());

    // Spawn the web server task
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

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(id, *stack, app, config));
    }
    info!("Tasks spawned");

    // Main loop
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[embassy_executor::task]
async fn net_runner_task(
    mut runner: embassy_net::Runner<'static, esp_wifi::wifi::WifiDevice<'static>>,
) {
    runner.run().await
}

fn initialize_appstate(board: Board) -> () {
    let d0 = Output::new(board.D0, Level::Low, OutputConfig::default());
    let d1 = Output::new(board.D1, Level::Low, OutputConfig::default());
    APP_STATE
        .set(RwLock::new(AppState { d0, d1 }))
        .ok()
        .expect("Failed to initialize app state");
}

const WEB_TASK_POOL_SIZE: usize = 4;
#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
async fn web_task(
    id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<Application>,
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
