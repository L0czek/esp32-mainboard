use core::net::Ipv4Addr;

use defmt::info;
use embassy_net::{Ipv4Cidr, Runner, StackResources, StaticConfigV4};
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_wifi::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration, ScanConfig, WifiController, WifiDevice, WifiEvent, WifiState
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
    esp_wifi_ctrl: &'static esp_wifi::EspWifiController<'static>,
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    rng: &mut Rng,
) -> WifiResources {
    // Initialize WiFi
    let (mut controller, interfaces) =
        esp_wifi::wifi::new(&esp_wifi_ctrl, wifi_peripheral).unwrap();

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
    let client_config = Configuration::Client(
        ClientConfiguration {
            ssid: WIFI_SSID.into(),
            password: WIFI_PASSWORD.into(),
            ..Default::default()
        }
    );
    controller.set_configuration(&client_config).unwrap();

    // Spawn WiFi tasks
    spawner.spawn(connection_task(controller)).unwrap();
    spawner.spawn(net_task(sta_runner)).unwrap();

    WifiResources {
        sta_stack,
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    info!("start connection task");
    controller.start_async().await.unwrap();
    controller.connect_async().await.unwrap();
}

// spawned for both ap and sta interfaces
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
