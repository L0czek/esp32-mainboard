//! Digital I/O module with configurable pin modes
//! 
//! This module provides a safe interface for controlling digital I/O pins using a command-driven
//! task pattern. Each pin is managed by its own task that processes commands
//! and monitors pin state changes.

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_futures::select;
use embassy_futures::select::Either;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{self, Watch};
use esp_hal::gpio::{AnyPin, DriveMode, Flex, Level, Output, OutputConfig, OutputPin};

use crate::channel::RequestResponseChannel;

// ============================================================================
// TYPES
// ============================================================================

/// Pin drive mode
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PinMode {
    /// Open-drain output - can pull low or float high. When floating, can read external signals.
    OpenDrain,
    /// Push-pull output - can drive high or low
    PushPull,
}

impl PinMode {
    pub fn to_str(&self) -> &'static str {
        match self {
            PinMode::OpenDrain => "OpenDrain",
            PinMode::PushPull => "PushPull",
        }
    }
}

/// Represents the actual state of a pin
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PinState {
    InLow,      // Pin is reading as low (0V)
    InHigh,     // Pin is reading as high (VCC)
    DrivingLow,  // Pin is being pulled down
    DrivingHigh, // Pin is being pulled up
    
    // Error state
    FunckingBad, // Invalid or undetermined state
}

impl PinState {
    pub fn to_str(&self) -> &'static str {
        match self {
            PinState::InLow => "In Low",
            PinState::InHigh => "In High",
            PinState::DrivingLow => "Driving Low",
            PinState::DrivingHigh => "Driving High",
            PinState::FunckingBad => "Fucking Bad (short circuit!)",
        }
    }
}

/// Identifies which digital pin to control
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DigitalPinID {
    D0,
    D1,
    D2,
    D3,
    D4,
}

/// Commands that can be sent to the pin task
enum Command {
    /// Set pin level (interpretation depends on current mode)
    /// In OpenDrain: false=pull down, true=float
    /// In PushPull: false=drive low, true=drive high
    SetState(bool),
    /// Change the pin mode
    SetMode(PinMode),
}

type CommandResult = ();

// ============================================================================
// CHANNELS
// ============================================================================

/// Channels for sending commands to output tasks
static DIGITAL_D0_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static DIGITAL_D1_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static DIGITAL_D2_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static DIGITAL_D3_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static DIGITAL_D4_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();

/// Watch channels for pin (mode, state) notifications
static DIGITAL_D0_STATE: Watch<CriticalSectionRawMutex, (PinMode, PinState), 4> = Watch::new();
static DIGITAL_D1_STATE: Watch<CriticalSectionRawMutex, (PinMode, PinState), 4> = Watch::new();
static DIGITAL_D2_STATE: Watch<CriticalSectionRawMutex, (PinMode, PinState), 4> = Watch::new();
static DIGITAL_D3_STATE: Watch<CriticalSectionRawMutex, (PinMode, PinState), 4> = Watch::new();
static DIGITAL_D4_STATE: Watch<CriticalSectionRawMutex, (PinMode, PinState), 4> = Watch::new();

pub type DigitalPinStateReceiver = watch::Receiver<'static, CriticalSectionRawMutex, (PinMode, PinState), 4>;

static DIGITAL_IO_STARTED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SPAWN METHOD
// ============================================================================

/// Initialize the digital IO module
pub fn spawn_digital_io(
    spawner: &Spawner,
    d0: impl OutputPin + 'static,
    d1: impl OutputPin + 'static,
    d2: impl OutputPin + 'static,
    d3: impl OutputPin + 'static,
    d4: impl OutputPin + 'static,
) -> DigitalIoHandle {
    if DIGITAL_IO_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        panic!("digital IO already started");
    }

    // Spawn tasks for each pin (all start in OpenDrain mode, floating high)
    spawner.spawn(digital_pin_task(DigitalPinID::D0, d0.degrade(), PinMode::OpenDrain, true)).expect("spawn digital D0 failed");
    spawner.spawn(digital_pin_task(DigitalPinID::D1, d1.degrade(), PinMode::OpenDrain, true)).expect("spawn digital D1 failed");
    spawner.spawn(digital_pin_task(DigitalPinID::D2, d2.degrade(), PinMode::OpenDrain, true)).expect("spawn digital D2 failed");
    spawner.spawn(digital_pin_task(DigitalPinID::D3, d3.degrade(), PinMode::OpenDrain, true)).expect("spawn digital D3 failed");
    spawner.spawn(digital_pin_task(DigitalPinID::D4, d4.degrade(), PinMode::OpenDrain, true)).expect("spawn digital D4 failed");

    DigitalIoHandle { _priv: PhantomData }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Helper to compute pin state from electrical readings and mode
fn pin_state(pin: &Flex<'_>, mode: PinMode) -> PinState {
    let is_high = pin.is_high();
    let is_set_high = pin.is_set_high();
    
    match (is_high, is_set_high, mode) {
        // Open-drain mode
        (true, true, PinMode::OpenDrain) => PinState::InHigh,  // Floating, reading high
        (false, true, PinMode::OpenDrain) => PinState::InLow,  // Floating, reading low
        (false, false, PinMode::OpenDrain) => PinState::DrivingLow,
        (true, false, PinMode::OpenDrain) => PinState::FunckingBad, // Shouldn't happen
        
        // Push-pull mode
        (true, true, PinMode::PushPull) => PinState::DrivingHigh,
        (false, false, PinMode::PushPull) => PinState::DrivingLow,
        (false, true, PinMode::PushPull) => PinState::FunckingBad,
        (true, false, PinMode::PushPull) => PinState::FunckingBad,
    }
}

// ============================================================================
// TASK
// ============================================================================

/// Main task that manages a pin
#[embassy_executor::task(pool_size = 5)]
async fn digital_pin_task(output_id: DigitalPinID, pin: AnyPin<'static>, initial_mode: PinMode, initial_state: bool) {
    let (channel, pin_state_watch) = match output_id {
        DigitalPinID::D0 => (&DIGITAL_D0_CHANNEL, &DIGITAL_D0_STATE),
        DigitalPinID::D1 => (&DIGITAL_D1_CHANNEL, &DIGITAL_D1_STATE),
        DigitalPinID::D2 => (&DIGITAL_D2_CHANNEL, &DIGITAL_D2_STATE),
        DigitalPinID::D3 => (&DIGITAL_D3_CHANNEL, &DIGITAL_D3_STATE),
        DigitalPinID::D4 => (&DIGITAL_D4_CHANNEL, &DIGITAL_D4_STATE),
    };
    let sender = pin_state_watch.sender();

    // Configure pin with initial mode and state
    let mut pin = Output::new(
        pin,
        match initial_state {
            true => Level::High,
            false => Level::Low,
        },
        OutputConfig::default().with_drive_mode(match initial_mode {
            PinMode::OpenDrain => DriveMode::OpenDrain,
            PinMode::PushPull => DriveMode::PushPull,
        }),
    ).into_flex();
    pin.set_input_enable(true);

    let mut current_mode = initial_mode;
    loop {
        // Send the current state
        sender.send((current_mode, pin_state(&pin, current_mode)));

        // Wait for either a command or a pin edge
        match select::select(channel.recv_request(), pin.wait_for_any_edge()).await {
            // Handle command
            Either::First(command) => {
                match command {
                    Command::SetState(state) => {
                        pin.set_level(state.into());
                        channel.send_response(()).await;
                    },
                    Command::SetMode(mode) => {
                        current_mode = mode;
                        pin.apply_output_config(
                            &OutputConfig::default().with_drive_mode(match current_mode {
                                PinMode::OpenDrain => DriveMode::OpenDrain,
                                PinMode::PushPull => DriveMode::PushPull,
                            })
                        );
                        channel.send_response(()).await;
                    },
                }
            },
            
            // Handle pin edge
            Either::Second(_) => {
                // do nothing, just update the state
            },
        }
    }
}

// ============================================================================
// HANDLE
// ============================================================================

#[derive(Clone, Copy)]
pub struct DigitalIoHandle {
    _priv: PhantomData<()>,
}

impl DigitalIoHandle {
    /// Set the output state
    /// 
    /// # Arguments
    /// * `output_id` - Which output to control
    /// * `state` - Interpretation depends on current mode:
    ///   - OpenDrain: false=pull down, true=float (can be used to read external signal)
    ///   - PushPull: false=drive low, true=drive high
    pub async fn set(&self, output_id: DigitalPinID, state: bool) {
        match output_id {
            DigitalPinID::D0 => DIGITAL_D0_CHANNEL.transact(Command::SetState(state)).await,
            DigitalPinID::D1 => DIGITAL_D1_CHANNEL.transact(Command::SetState(state)).await,
            DigitalPinID::D2 => DIGITAL_D2_CHANNEL.transact(Command::SetState(state)).await,
            DigitalPinID::D3 => DIGITAL_D3_CHANNEL.transact(Command::SetState(state)).await,
            DigitalPinID::D4 => DIGITAL_D4_CHANNEL.transact(Command::SetState(state)).await,
        }
    }

    /// Set the pin mode
    /// 
    /// # Arguments
    /// * `output_id` - Which pin to configure
    /// * `mode` - The desired mode (Input, OpenDrain, or PushPull)
    pub async fn set_mode(&self, output_id: DigitalPinID, mode: PinMode) {
        match output_id {
            DigitalPinID::D0 => DIGITAL_D0_CHANNEL.transact(Command::SetMode(mode)).await,
            DigitalPinID::D1 => DIGITAL_D1_CHANNEL.transact(Command::SetMode(mode)).await,
            DigitalPinID::D2 => DIGITAL_D2_CHANNEL.transact(Command::SetMode(mode)).await,
            DigitalPinID::D3 => DIGITAL_D3_CHANNEL.transact(Command::SetMode(mode)).await,
            DigitalPinID::D4 => DIGITAL_D4_CHANNEL.transact(Command::SetMode(mode)).await,
        }
    }

    /// Get a receiver that will be notified when the specified pin's state or mode changes
    pub fn watch(
        &self,
        id: DigitalPinID,
    ) -> Option<DigitalPinStateReceiver> {
        match id {
            DigitalPinID::D0 => DIGITAL_D0_STATE.receiver(),
            DigitalPinID::D1 => DIGITAL_D1_STATE.receiver(),
            DigitalPinID::D2 => DIGITAL_D2_STATE.receiver(),
            DigitalPinID::D3 => DIGITAL_D3_STATE.receiver(),
            DigitalPinID::D4 => DIGITAL_D4_STATE.receiver(),
        }
    }

    /// Get the current state and mode of a pin
    /// Note: Prefer watch() for updates instead of polling with this function
    pub fn get(&self, id: DigitalPinID) -> Option<(PinMode, PinState)> {
        match id {
            DigitalPinID::D0 => DIGITAL_D0_STATE.try_get(),
            DigitalPinID::D1 => DIGITAL_D1_STATE.try_get(),
            DigitalPinID::D2 => DIGITAL_D2_STATE.try_get(),
            DigitalPinID::D3 => DIGITAL_D3_STATE.try_get(),
            DigitalPinID::D4 => DIGITAL_D4_STATE.try_get(),
        }
    }
}
