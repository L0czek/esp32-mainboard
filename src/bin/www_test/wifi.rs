use core::net::Ipv4Addr;

use defmt::info;
use embassy_net::{Ipv4Cidr, Runner, StackResources, StaticConfigV4};
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_wifi::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration, WifiController,
    WifiDevice, WifiEvent, WifiState,
};
use rand_core::RngCore as _;
use static_cell::StaticCell;

use crate::config::{AP_PASSWORD, AP_SSID, WIFI_PASSWORD, WIFI_SSID};

// Shared resources
pub static AP_STACK_RESOURCES: StaticCell<StackResources<20>> = StaticCell::new();
pub static STA_STACK_RESOURCES: StaticCell<StackResources<20>> = StaticCell::new();

pub struct WifiResources {
    pub ap_stack: embassy_net::Stack<'static>,
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
    let wifi_ap_device = interfaces.ap;
    let wifi_sta_device = interfaces.sta;

    // Configure AP with static IP and STA with DHCP
    let ap_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Addr::new(192, 168, 2, 1), 24),
        gateway: Some(Ipv4Addr::new(192, 168, 2, 1)),
        dns_servers: Default::default(),
    });
    let sta_config = embassy_net::Config::dhcpv4(Default::default());

    // Generate seed for network stacks
    let seed = rng.next_u64();

    // Initialize network stacks
    let (ap_stack, ap_runner) = embassy_net::new(
        wifi_ap_device,
        ap_config,
        AP_STACK_RESOURCES.init(StackResources::<20>::new()),
        seed,
    );
    let (sta_stack, sta_runner) = embassy_net::new(
        wifi_sta_device,
        sta_config,
        STA_STACK_RESOURCES.init(StackResources::<20>::new()),
        seed,
    );

    // Configure WiFi in mixed mode
    let client_config = Configuration::Mixed(
        ClientConfiguration {
            ssid: WIFI_SSID.into(),
            password: WIFI_PASSWORD.into(),
            ..Default::default()
        },
        AccessPointConfiguration {
            ssid: AP_SSID.into(),
            password: AP_PASSWORD.into(),
            auth_method: AuthMethod::WPA2Personal,
            ..Default::default()
        },
    );
    controller.set_configuration(&client_config).unwrap();

    // Spawn WiFi tasks
    spawner.spawn(connection_task(controller)).unwrap();
    spawner.spawn(net_task(ap_runner)).unwrap();
    spawner.spawn(net_task(sta_runner)).unwrap();
    // Wait for AP to come up
    loop {
        if ap_stack.is_link_up() {
            info!("AP is up at 192.168.2.1");
            break;
        }
        info!("Waiting for AP to come up...");
        Timer::after(Duration::from_millis(500)).await;
    }
    info!(
        "Connect to AP `{}` with password `{}`",
        AP_SSID, AP_PASSWORD
    );

    info!("You can connect to your router and access via WiFi STA IP also now");

    WifiResources {
        ap_stack,
        sta_stack,
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    info!("Starting WiFi connection task");

    info!("Starting WiFi");
    controller.start_async().await.unwrap();
    info!("WiFi started!");

    loop {
        match esp_wifi::wifi::ap_state() {
            WifiState::ApStarted => {
                info!("About to connect to WiFi...");

                match controller.connect_async().await {
                    Ok(_) => {
                        // Wait until we're no longer connected
                        controller.wait_for_event(WifiEvent::StaDisconnected).await;
                        info!("STA disconnected");
                    }
                    Err(e) => {
                        info!("Failed to connect to WiFi: {:?}", e);
                        Timer::after(Duration::from_millis(5000)).await
                    }
                }
            }
            _ => return,
        }
    }
}

// spawned for both ap and sta interfaces
#[embassy_executor::task(pool_size = 2)]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
