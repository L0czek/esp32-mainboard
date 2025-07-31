use defmt::info;
use embassy_time::Duration;
use picoserve::{
    make_static,
    response::{ErrorWithStatusCode, File, IntoResponse},
    routing::{self, get, parse_path_segment, PathRouter},
    AppBuilder, AppRouter, Router,
};
use serde::Serialize;
extern crate alloc;
use crate::{server::alloc::string::ToString, simple_output::set_state};
use alloc::string::String;

use crate::wifi::WifiResources;
use crate::{
    html::INDEX_HTML,
    simple_output::{get_states, OutputID},
};
use bq24296m;
use mainboard::board::POWER_CONTROL;
use mainboard::tasks::{PowerRequest, PowerResponse};

// Define the pool size for web tasks
const WEB_TASK_POOL_SIZE: usize = 8;

#[derive(Serialize)]
struct APIResponse {
    ok: bool,
}

#[derive(Serialize)]
struct PinStatesResponse<'a> {
    ok: bool,
    pin0_state: &'a str,
    pin1_state: &'a str,
}

#[derive(Serialize)]
struct PowerStatsResponse<'a> {
    ok: bool,
    // Status register decoded values
    vbus_status: &'a str,
    charge_status: &'a str,
    dpm_active: bool,
    power_good: bool,
    thermal_regulation_active: bool,
    vsys_regulation_active: bool,
    // Fault register decoded values
    watchdog_fault: bool,
    otg_fault: bool,
    charge_fault_status: &'a str,
    battery_fault: bool,
    ntc_fault_status: &'a str,
    ntc_cold_fault: bool,
    ntc_hot_fault: bool,
    // Boost converter state
    boost_converter_enabled: bool,
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
            .route("/api/power/stats", get(power_stats_handler))
            .route(
                ("/api/power/boost", parse_path_segment::<bool>()),
                get(power_boost_handler),
            )
            .route("/api/pins/states", get(pin_states_handler))
    }
}

#[derive(Debug, thiserror::Error, ErrorWithStatusCode)]
#[status_code(BAD_REQUEST)]
enum BadRequest {
    #[error("Bad: {0}")]
    Bad(String),
}

// Handler for digital I/O API
async fn digital_handler((id, value): (u8, u8)) -> impl IntoResponse {
    info!("Setting digital pin {} to {}", id, value);
    let pin: OutputID = match id {
        0 => OutputID::Output1,
        1 => OutputID::Output2,
        _ => return Err(BadRequest::Bad("Invalid pin".to_string())),
    };

    let value: bool = match value {
        0 | 1 => value != 0,
        _ => return Err(BadRequest::Bad("Invalid value".to_string())),
    };
    set_state(pin, value).await;
    // Here you'd implement actual pin control
    // For now, just return a success message
    Ok(picoserve::response::Json(APIResponse { ok: true }))
}

/// Handler for boost converter control
async fn power_boost_handler(enable: bool) -> impl IntoResponse {
    info!(
        "Setting boost converter to: {}",
        if enable { "enabled" } else { "disabled" }
    );

    // Send command to power controller
    let response = match POWER_CONTROL
        .transact(PowerRequest::EnableBoostConverter(enable))
        .await
    {
        PowerResponse::Ok => {
            info!("Boost converter set successfully");
            APIResponse { ok: true }
        }
        PowerResponse::Err(_) => {
            info!("Failed to set boost converter state");
            return Err(BadRequest::Bad("Failed to set boost converter".to_string()));
        }
        _ => {
            info!("Unexpected power controller response");
            return Err(BadRequest::Bad(
                "Unexpected power controller response".to_string(),
            ));
        }
    };

    Ok(picoserve::response::Json(response))
}

/// Handler for getting pin states
async fn pin_states_handler() -> impl IntoResponse {
    info!("Getting pin states");

    // Get pin states from simple_output module
    let (pin0, pin1) = get_states().await;

    let pin0_state = pin0.to_str();
    let pin1_state = pin1.to_str();

    // Create and return the response
    let response = PinStatesResponse {
        ok: true,
        pin0_state,
        pin1_state,
    };

    picoserve::response::Json(response)
}

/// Handler for power controller stats
async fn power_stats_handler() -> impl IntoResponse {
    info!("Getting power controller stats");

    // Request power controller stats
    let response = match POWER_CONTROL.transact(PowerRequest::GetStats).await {
        PowerResponse::Status(stats) => {
            // Use helper functions to decode register values
            let status = &stats.charger_status;
            let faults = &stats.charger_faults;

            info!("Power stats: retrieved successfully");

            // Convert enum values to static string slices to avoid lifetime issues
            let vbus_status = match status.get_vbus_status() {
                bq24296m::VbusStatus::Unknown => "Unknown",
                bq24296m::VbusStatus::UsbHost => "USB Host",
                bq24296m::VbusStatus::AdapterPort => "Adapter Port",
                bq24296m::VbusStatus::Otg => "OTG",
            };

            let charge_status = match status.get_charge_status() {
                bq24296m::ChargeStatus::NotCharging => "Not Charging",
                bq24296m::ChargeStatus::PreCharge => "Pre-Charge",
                bq24296m::ChargeStatus::FastCharging => "Fast Charging",
                bq24296m::ChargeStatus::ChargeDone => "Charge Done",
            };

            let charge_fault_status = match faults.get_charge_fault_status() {
                bq24296m::ChargeFaultStatus::Normal => "Normal",
                bq24296m::ChargeFaultStatus::InputFault => "Input Fault",
                bq24296m::ChargeFaultStatus::ThermalShutdown => "Thermal Shutdown",
                bq24296m::ChargeFaultStatus::ChargeTimerExpired => "Charge Timer Expired",
            };

            let ntc_fault_status = match faults.get_ntc_fault_status() {
                bq24296m::NtcFaultStatus::Normal => "Normal",
                bq24296m::NtcFaultStatus::Cold => "Cold",
                bq24296m::NtcFaultStatus::Hot => "Hot",
                bq24296m::NtcFaultStatus::ColdAndHot => "Cold and Hot",
            };

            PowerStatsResponse {
                ok: true,
                // Status register decoded values
                vbus_status,
                charge_status,
                dpm_active: status.is_dpm_active(),
                power_good: status.is_power_good(),
                thermal_regulation_active: status.is_thermal_regulation_active(),
                vsys_regulation_active: status.is_vsys_regulation_active(),
                // Fault register decoded values
                watchdog_fault: faults.is_watchdog_fault(),
                otg_fault: faults.is_otg_fault(),
                charge_fault_status,
                battery_fault: faults.is_battery_fault(),
                ntc_fault_status,
                ntc_cold_fault: faults.is_ntc_cold_fault(),
                ntc_hot_fault: faults.is_ntc_hot_fault(),
                // Boost converter state
                boost_converter_enabled: stats.boost_enabled,
            }
        }
        PowerResponse::Err(_) => {
            info!("Failed to get power controller stats");
            return Err(BadRequest::Bad("Failed to get power stats".to_string()));
        }
        _ => {
            info!("Unexpected power controller response");
            return Err(BadRequest::Bad(
                "Unexpected power controller response".to_string(),
            ));
        }
    };

    Ok(picoserve::response::Json(response))
}

/// Initialize and run the web server
///
/// This function sets up the picoserve server using the provided WiFi resources
/// and spawns tasks to handle web requests.
pub async fn run_server(spawner: embassy_executor::Spawner, wifi_resources: &WifiResources) {
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
        spawner.spawn(web_task(id, *ap_stack, app, config)).unwrap();
    }

    // Start web tasks for STA interface
    for id in 0..WEB_TASK_POOL_SIZE {
        spawner
            .spawn(web_task(id + WEB_TASK_POOL_SIZE, *sta_stack, app, config))
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
