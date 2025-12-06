use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

use bq24296m::WatchdogTimer;
use defmt::{error, info};
use embassy_executor::Spawner;
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

// ============================================================================
// TYPES
// ============================================================================

pub enum PowerRequest {
    EnableBoostConverter(bool),
    CheckInterrupt,
    SetMode(PowerControllerMode),
}

pub enum PowerResponse {
    Ok,
    Err(PowerControllerError<I2cType>),
}

// ============================================================================
// CHANNELS
// ============================================================================

// Command channel for power control
static POWER_CONTROL: RequestResponseChannel<PowerRequest, PowerResponse, 16> =
    RequestResponseChannel::with_static_channels();

// Power state management
static POWER_STATE: watch::Watch<CriticalSectionRawMutex, PowerControllerStats, 4> = 
    watch::Watch::new();

pub type PowerStateReceiver = watch::Receiver<'static, CriticalSectionRawMutex, PowerControllerStats, 4>;

static POWER_STARTED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SPAWN METHOD
// ============================================================================

pub fn spawn_power_controller(
    spawner: &Spawner,
    config: PowerControllerConfig,
    io: PowerControllerIO<I2cType>,
) -> PowerHandle {
    if POWER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        panic!("power controller already started");
    }

    spawner
        .spawn(power_controller_task(config, io))
        .expect("spawn power controller failed");

    PowerHandle { _priv: PhantomData }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

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
        PowerRequest::SetMode(mode) => {
            match pctl.read_stats() {
                Ok(stats) => match pctl.switch_mode(mode, &stats) {
                    Ok(()) => PowerResponse::Ok,
                    Err(e) => PowerResponse::Err(e),
                },
                Err(e) => PowerResponse::Err(e),
            }
        }
    }
}

// ============================================================================
// TASK
// ============================================================================

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
            Timer::after_millis(50).await;
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
                Timer::after_millis(50).await;
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

// ============================================================================
// HANDLE
// ============================================================================

#[derive(Clone, Copy)]
pub struct PowerHandle {
    _priv: PhantomData<()>,
}

impl PowerHandle {
    pub async fn transact(&self, req: PowerRequest) -> PowerResponse {
        POWER_CONTROL.transact(req).await
    }

    pub async fn set_boost_converter(&self, enable: bool) -> PowerResponse {
        self.transact(PowerRequest::EnableBoostConverter(enable)).await
    }

    pub async fn set_mode(&self, mode: PowerControllerMode) -> PowerResponse {
        self.transact(PowerRequest::SetMode(mode)).await
    }

    pub async fn check_interrupt(&self) -> PowerResponse {
        self.transact(PowerRequest::CheckInterrupt).await
    }

    pub fn state_receiver(&self) -> Option<PowerStateReceiver> {
        POWER_STATE.receiver()
    }

    pub fn state(&self) -> Option<PowerControllerStats> {
        POWER_STATE.try_get()
    }
}
