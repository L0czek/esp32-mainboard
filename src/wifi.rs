use core::net::Ipv4Addr;

use defmt::info;
use embassy_net::{Ipv4Cidr, Runner, StackResources, StaticConfigV4};
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_radio::wifi::{
    AccessPointConfig, AuthMethod, ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent,
};
use rand_core::RngCore as _;
use static_cell::StaticCell;

use crate::config::{AP_PASSWORD, AP_SSID, WIFI_PASSWORD, WIFI_SSID};

// Shared resources
pub static AP_STACK_RESOURCES: StaticCell<StackResources<20>> = StaticCell::new();
pub static STA_STACK_RESOURCES: StaticCell<StackResources<20>> = StaticCell::new();

pub type WifiResourceSta = embassy_net::Stack<'static>;

/// Initialize WiFi in STA mode
/// Returns the WiFi resources needed by the server
pub async fn initialize_wifi_sta(
    spawner: embassy_executor::Spawner,
    esp_wifi_ctrl: &'static esp_radio::Controller<'static>,
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    rng: &mut Rng,
) -> WifiResourceSta {
    // Initialize WiFi
    let (mut controller, interfaces) =
        esp_radio::wifi::new(esp_wifi_ctrl, wifi_peripheral, Default::default()).unwrap();

    // Initialize network stacks
    let (sta_stack, sta_runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        STA_STACK_RESOURCES.init(StackResources::<20>::new()),
        rng.next_u64(),
    );

    // Configure WiFi in Station (STA) mode
    controller
        .set_config(&ModeConfig::Client(
            ClientConfig::default()
                .with_ssid(WIFI_SSID.into())
                .with_password(WIFI_PASSWORD.into()),
        ))
        .unwrap();

    // Spawn WiFi tasks
    spawner.spawn(connection_task(controller)).unwrap();
    spawner.spawn(net_task(sta_runner)).unwrap();

    sta_stack
}

pub struct WifiResourcesMixed {
    pub ap_stack: embassy_net::Stack<'static>,
    pub sta_stack: embassy_net::Stack<'static>,
}

/// Initialize WiFi in mixed mode (AP + STA)
/// Returns the WiFi resources needed by the server
pub async fn initialize_wifi_mixed(
    spawner: embassy_executor::Spawner,
    esp_wifi_ctrl: &'static esp_radio::Controller<'static>,
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    rng: &mut Rng,
) -> WifiResourcesMixed {
    // Initialize WiFi
    let (mut controller, interfaces) =
        esp_radio::wifi::new(esp_wifi_ctrl, wifi_peripheral, Default::default()).unwrap();

    // Initialize network stacks
    let (ap_stack, ap_runner) = embassy_net::new(
        interfaces.ap,
        embassy_net::Config::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(Ipv4Addr::new(192, 168, 2, 1), 24),
            gateway: Some(Ipv4Addr::new(192, 168, 2, 1)),
            dns_servers: Default::default(),
        }),
        AP_STACK_RESOURCES.init(StackResources::<20>::new()),
        rng.next_u64(),
    );
    let (sta_stack, sta_runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        STA_STACK_RESOURCES.init(StackResources::<20>::new()),
        rng.next_u64(),
    );

    // Configure WiFi in mixed mode (AP + STA)
    let mixed_config = ModeConfig::ApSta(
        ClientConfig::default()
            .with_ssid(WIFI_SSID.into())
            .with_password(WIFI_PASSWORD.into()),
        AccessPointConfig::default()
            .with_ssid(AP_SSID.into())
            .with_password(AP_PASSWORD.into())
            .with_auth_method(AuthMethod::Wpa2Personal),
    );
    controller.set_config(&mixed_config).unwrap();

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

    WifiResourcesMixed {
        ap_stack,
        sta_stack,
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    info!("Starting WiFi connection task");
    info!("Device capabilities: {:?}", controller.capabilities());
    controller.start_async().await.unwrap();

    loop {
        if matches!(controller.is_started(), Ok(true)) {
            info!("Connecting to {}", WIFI_SSID);
            match controller.connect_async().await {
                Ok(_) => {
                    info!("Connected to {}", WIFI_SSID);
                    // Wait until we're no longer connected
                    controller.wait_for_event(WifiEvent::StaDisconnected).await;
                    info!("STA disconnected");
                }
                Err(e) => {
                    info!("Failed to connect to WiFi: {:?}", e);
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
