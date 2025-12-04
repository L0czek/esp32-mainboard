use bq24296m::WatchdogTimer;
use defmt::{error, info};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch;
use embassy_time::Timer;

use crate::{
    channel::RequestResponseChannel,
    power::{
        PowerController, PowerControllerConfig, PowerControllerError, PowerControllerIO,
        PowerControllerMode, PowerControllerStats
    },
    I2cType,
};

pub enum PowerRequest {
    EnableBoostConverter(bool),
    CheckInterrupt,
}

pub enum PowerResponse {
    Ok,
    Err(PowerControllerError<I2cType>),
}

fn handle_power_controller_interrupt(
    pctl: &mut PowerController<I2cType>,
) -> Result<(), PowerControllerError<I2cType>> {
    let stats = pctl.read_stats()?;

    // If VBUS is not present and we are not in OTG mode, enter OTG mode
    // If VBUS is present and we are in OTG mode, switch to charging mode
    match pctl.get_mode() {
        PowerControllerMode::Otg => {
            if stats.expander_status.vbus_present() {
                info!("VBUS present, switching to Charging mode");
                pctl.switch_mode(PowerControllerMode::Charging, &stats)?;
            }
        }
        _ => {
            if !stats.expander_status.vbus_present() {
                info!("VBUS not present, switching to OTG mode");
                pctl.switch_mode(PowerControllerMode::Otg, &stats)?;
            }
        }
    }

    Ok(())
}

fn handle_power_controller_command(
    pctl: &mut PowerController<I2cType>,
    command: PowerRequest,
) -> PowerResponse {
    match command {
        PowerRequest::EnableBoostConverter(true) => {
            pctl.enable_boost_converter();
            PowerResponse::Ok
        }
        PowerRequest::EnableBoostConverter(false) => {
            pctl.disable_boost_converter();
            PowerResponse::Ok
        }
        PowerRequest::CheckInterrupt => {
            match handle_power_controller_interrupt(pctl) {
                Ok(()) => PowerResponse::Ok,
                Err(e) => PowerResponse::Err(e),
            }
        }
    }
}

#[embassy_executor::task]
pub async fn power_controller_task(
    config: PowerControllerConfig,
    io: PowerControllerIO<I2cType>,
) {
    let ping_time = config.i2c_watchdog_timer;
    let mut pctl = match PowerController::new(config, io) {
        Ok(controller) => controller,
        Err(e) => {
            error!("Failed to initialize power controller: {:?}", e);
            return;
        }
    };

    let sleep_time = match ping_time {
        WatchdogTimer::Disabled | WatchdogTimer::Seconds40 => 20,
        WatchdogTimer::Seconds80 => 40,
        WatchdogTimer::Seconds160 => 80,
    };

    let mut initial_mode_set = false;

    loop {
        let stats = if let Ok(stats) = pctl.read_stats() {
            POWER_STATE.sender().send(stats.clone());
            stats
        } else {
            error!("Failed to read charger stats");
            continue;
        };

        // Set initial mode based on VBUS presence on first successful stats read
        if !initial_mode_set {
            let initial_mode = if stats.expander_status.vbus_present() {
                PowerControllerMode::Charging
            } else {
                PowerControllerMode::Otg
            };
            if let Err(e) = pctl.switch_mode(initial_mode, &stats) {
                error!("Failed to set initial mode: {:?}", e);
                continue;
            }
            initial_mode_set = true;
        }

        let timeout = Timer::after_secs(sleep_time);
        let command = POWER_CONTROL.recv_request();

        let result = select(timeout, command).await;

        if let Either::Second(cmd) = result {
            let response = handle_power_controller_command(&mut pctl, cmd);
            POWER_CONTROL.send_response(response).await;
        }

        if let Err(e) = pctl.reset_watchdog() {
            error!("Failed to reset watchdog: {:?}", e);
        } else {
            info!("Charger watchdog reset");
        }
    }
}

// Command channel for power control
pub static POWER_CONTROL: RequestResponseChannel<PowerRequest, PowerResponse, 16> =
    RequestResponseChannel::with_static_channels();

// Power state management
pub static POWER_STATE: watch::Watch<CriticalSectionRawMutex, PowerControllerStats, 4> = 
    watch::Watch::new();
