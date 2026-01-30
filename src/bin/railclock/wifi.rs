
use defmt::info;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_radio::wifi::{
    ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent
};
use rand_core::RngCore as _;
use static_cell::StaticCell;

use crate::config::{WIFI_PASSWORD, WIFI_SSID};

// Shared resources
pub static STA_STACK_RESOURCES: StaticCell<StackResources<20>> = StaticCell::new();

pub struct WifiResources {
    pub sta_stack: embassy_net::Stack<'static>,
}

/// Initialize WiFi in mixed mode (AP + STA)
/// Returns the WiFi resources needed by the server
pub async fn initialize_wifi(
    spawner: embassy_executor::Spawner,
    esp_wifi_ctrl: &'static esp_radio::Controller<'static>,
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    rng: &mut Rng,
) -> WifiResources {
    // Initialize WiFi
    let (mut controller, interfaces) =
        esp_radio::wifi::new(esp_wifi_ctrl, wifi_peripheral, Default::default()).unwrap();

    // Get WiFi devices
    let wifi_sta_device = interfaces.sta;

    // Configure AP with static IP and STA with DHCP
    let sta_config = embassy_net::Config::dhcpv4(Default::default());

    // Generate seed for network stacks
    let seed = rng.next_u64();

    // Initialize network stacks
    let (sta_stack, sta_runner) = embassy_net::new(
        wifi_sta_device,
        sta_config,
        STA_STACK_RESOURCES.init(StackResources::<20>::new()),
        seed,
    );

    // Configure WiFi in mixed mode
    let station_config = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(WIFI_SSID.into())
            .with_password(WIFI_PASSWORD.into())
    );
    controller.set_config(&station_config).unwrap();

    // Spawn WiFi tasks
    spawner.spawn(connection_task(controller)).unwrap();
    spawner.spawn(net_task(sta_runner)).unwrap();

    loop {
        if let Some(config) = sta_stack.config_v4() {
            let address = config.address.address();
             info!("Got IP: {}", address);
            break;
        }
        info!("Waiting for IP...");
        Timer::after(Duration::from_millis(500)).await;
    };


    WifiResources {
        sta_stack,
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    info!("start connection task");
    info!("Device capabilities: {:?}", controller.capabilities());

    info!("Starting wifi");
    controller.start_async().await.unwrap();
    info!("Wifi started!");

    loop {
        if matches!(controller.is_started(), Ok(true)) {
            info!("About to connect...");

            match controller.connect_async().await {
                Ok(_) => {
                    // wait until we're no longer connected
                    controller
                        .wait_for_event(WifiEvent::StaDisconnected)
                        .await;
                    info!("Station disconnected");
                }
                Err(e) => {
                    info!("Failed to connect to wifi: {:?}", e);
                    Timer::after(Duration::from_millis(5000)).await
                }
            }
        } else {
            return;
        }
    }
}

// spawned for both ap and sta interfaces
#[embassy_executor::task(pool_size = 2)]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
