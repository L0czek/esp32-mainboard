#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use mainboard::simple_output::initialize_simple_output;
use mainboard::{create_board, server, wifi, Board};

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use panic_rtt_target as _;
use static_cell::StaticCell;

extern crate alloc;

// StaticCell for WiFi controller
static ESP_WIFI_CTRL: StaticCell<esp_wifi::EspWifiController<'static>> = StaticCell::new();

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // Initialize RTT for logging
    rtt_target::rtt_init_defmt!();

    // Configure and initialize hardware
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let board = create_board!(peripherals);

    // Initialize heap allocator
    esp_alloc::heap_allocator!(size: 64 * 1024);

    // Initialize embassy time
    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);
    info!("Embassy initialized!");

    // Initialize RNG and timer for WiFi
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    // Initialize esp-radio controller
    let esp_wifi_ctrl = ESP_WIFI_CTRL.init(esp_wifi::init(timer1.timer0, rng).unwrap());

    // Initialize WiFi in mixed mode (AP + STA)
    info!("Initializing WiFi...");
    let wifi_resources =
        wifi::initialize_wifi(spawner, esp_wifi_ctrl, peripherals.WIFI, &mut rng).await;
    info!("WiFi initialized!");

    // Initialize simple output
    initialize_simple_output(board.D0, board.D1);

    // Start the web server
    info!("Starting web server...");
    server::run_server(spawner, &wifi_resources).await;
    info!("Web server started!");

    // Main loop
    loop {
        info!(
            "Server running... AP IP: {:?}, STA IP: {:?}",
            wifi_resources.ap_stack.config_v4().map(|c| c.address),
            wifi_resources.sta_stack.config_v4().map(|c| c.address)
        );
        Timer::after(Duration::from_secs(10)).await;
    }
}
