use bq24296m::{ChargeStatus, WatchdogTimer};
use defmt::{error, info};
use embassy_futures::select::{select, Either};
use embassy_time::Timer;

use crate::{
    board::POWER_CONTROL, error::AnyError, power::{PowerController, PowerControllerConfig, PowerControllerError, PowerControllerIO, PowerControllerMode, PowerControllerStats}, I2cType
};

pub enum PowerRequest {
    SwitchMode(PowerControllerMode),
    EnableBoostConverter(bool),
    CheckInterrupt,
    GetStats,
}

pub enum PowerResponse {
    Ok,
    Status(PowerControllerStats),
    Err(PowerControllerError<I2cType>)
}

#[embassy_executor::task]
pub async fn handle_power_controller(
    config: PowerControllerConfig,
    io: PowerControllerIO<I2cType>,
) {
    match handle_power_controller_impl(config, io).await {
        Ok(()) => info!("Charger task finished"),
        Err(e) => error!("Charger task failed, {:?}", e),
    }
}

fn handle_power_controller_interrupt(pctl: &mut PowerController<I2cType>) -> Result<(), PowerControllerError<I2cType>> {
    let stats = pctl.read_stats()?;

    //TODO: add charging interrupt handling

    match stats.charger_status.get_charge_status() {
        ChargeStatus::ChargeDone => pctl.switch_mode(PowerControllerMode::Passive)?,
        _ => {}
    }

    Ok(())
}

fn handle_power_controller_command(pctl: &mut PowerController<I2cType>, command: PowerRequest) -> PowerResponse {
    match command {
        PowerRequest::SwitchMode(mode) => match pctl.switch_mode(mode) {
            Ok(()) => PowerResponse::Ok,
            Err(e) => PowerResponse::Err(e)
        }
        PowerRequest::EnableBoostConverter(true) => {
            pctl.enable_boost_converter();
            PowerResponse::Ok
        }
        PowerRequest::EnableBoostConverter(false) => {
            pctl.disable_boost_converter();
            PowerResponse::Ok
        }
        PowerRequest::GetStats => match pctl.read_stats() {
            Ok(stats) => PowerResponse::Status(stats),
            Err(e) => PowerResponse::Err(e)
        }
        PowerRequest::CheckInterrupt => {
            // TODO: expand this logic
            match handle_power_controller_interrupt(pctl) {
                Ok(()) => PowerResponse::Ok,
                Err(e) => PowerResponse::Err(e)
            }

        }
    }
}

async fn handle_power_controller_impl(
    config: PowerControllerConfig,
    io: PowerControllerIO<I2cType>,
) -> Result<(), AnyError> {
    let ping_time = config.i2c_watchdog_timer;
    let mut pctl = PowerController::new(config, io)?;

    pctl.switch_mode(crate::power::PowerControllerMode::Charging)?;

    let sleep_time = match ping_time {
        WatchdogTimer::Disabled | WatchdogTimer::Seconds40 => 20,
        WatchdogTimer::Seconds80 => 40,
        WatchdogTimer::Seconds160 => 80,
    };

    loop {
        let timeout = Timer::after_secs(sleep_time);
        let command = POWER_CONTROL.recv_request();

        let result = select(timeout, command).await;

        if let Either::Second(cmd) = result {
            let response = handle_power_controller_command(&mut pctl, cmd);
            POWER_CONTROL.send_response(response).await;
        }

        pctl.reset_watchdog()?;
        info!("Charger watchdog reset");

        let stats = pctl.read_stats()?;
        stats.dump();
    }
}
