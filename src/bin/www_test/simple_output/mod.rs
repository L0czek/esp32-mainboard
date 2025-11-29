//! Simple output module with command-driven task pattern
//! 
//! This module provides a safe interface for controlling output pins using a command-driven
//! task pattern. Each output pin is managed by its own task that processes commands
//! and monitors pin state changes.

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{self, Watch};
use embassy_futures::select;
use embassy_futures::select::Either;
use esp_hal::gpio::{DriveMode, Flex, Level, Output, OutputConfig, OutputPin};
use mainboard::channel::RequestResponseChannel;

/// Represents the actual state of a pin as an input
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PinState {
    InLow,      // Pin is reading as low (0V)
    InHigh,     // Pin is reading as high (VCC)
    PullingDown, // Pin is being pulled down
    FunckingBad, // Invalid or undetermined state
}

impl PinState {
    pub fn to_str(&self) -> &'static str {
        match self {
            PinState::InLow => "In Low",
            PinState::InHigh => "In High",
            PinState::PullingDown => "Pulling Down",
            PinState::FunckingBad => "Fucking Bad (short circuit!)",
        }
    }
}

fn pin_state(pin: &Flex<'_>) -> PinState {
    match (pin.is_high(), pin.is_set_low()) {
        (true, false) => PinState::InHigh,
        (false, false) => PinState::InLow,
        (false, true) => PinState::PullingDown,
        (true, true) => PinState::FunckingBad,
    }
}

/// Identifies which output to control
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutputID {
    OutputD0,
    OutputD1,
    OutputD2,
    OutputD3,
    OutputD4,
}

/// Commands that can be sent to the output task
enum Command {
    SetState(bool),
}

type CommandResult = ();

/// Channels for sending commands to output tasks
static OUTPUT_D0_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static OUTPUT_D1_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static OUTPUT_D2_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static OUTPUT_D3_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();
static OUTPUT_D4_CHANNEL: RequestResponseChannel<Command, CommandResult, 4> = RequestResponseChannel::with_static_channels();

/// Watch channels for pin state (input) notifications
static OUTPUT_D0_STATE: Watch<CriticalSectionRawMutex, PinState, 4> = Watch::new();
static OUTPUT_D1_STATE: Watch<CriticalSectionRawMutex, PinState, 4> = Watch::new();
static OUTPUT_D2_STATE: Watch<CriticalSectionRawMutex, PinState, 4> = Watch::new();
static OUTPUT_D3_STATE: Watch<CriticalSectionRawMutex, PinState, 4> = Watch::new();
static OUTPUT_D4_STATE: Watch<CriticalSectionRawMutex, PinState, 4> = Watch::new();

/// Initialize the simple output module
pub fn initialize_simple_output(
    spawner: &Spawner,
    d0: impl OutputPin + 'static,
    d1: impl OutputPin + 'static,
    d2: impl OutputPin + 'static,
    d3: impl OutputPin + 'static,
    d4: impl OutputPin + 'static,
) {
    // Configure pins as open-drain outputs
    let output1 = new_configured_output(d0);
    let output2 = new_configured_output(d1);
    let output3 = new_configured_output(d2);
    let output4 = new_configured_output(d3);
    let output5 = new_configured_output(d4);
    // Spawn tasks for each output
    spawner.spawn(output_task(OutputID::OutputD0, output1)).unwrap();
    spawner.spawn(output_task(OutputID::OutputD1, output2)).unwrap();
    spawner.spawn(output_task(OutputID::OutputD2, output3)).unwrap();
    spawner.spawn(output_task(OutputID::OutputD3, output4)).unwrap();
    spawner.spawn(output_task(OutputID::OutputD4, output5)).unwrap();
}

/// Configure a pin as an open-drain output
fn new_configured_output<Pin: OutputPin + 'static>(pin: Pin) -> Flex<'static> {
    let mut gpio = Output::new(
        pin,
        Level::High,
        OutputConfig::default().with_drive_mode(DriveMode::OpenDrain),
    ).into_flex();
    gpio.set_input_enable(true);
    gpio
}

/// Main task that manages an output pin
#[embassy_executor::task(pool_size = 5)]
async fn output_task(output_id: OutputID, mut pin: Flex<'static>) -> ! {
    let (channel, pin_state_watch) = match output_id {
        OutputID::OutputD0 => (&OUTPUT_D0_CHANNEL, &OUTPUT_D0_STATE),
        OutputID::OutputD1 => (&OUTPUT_D1_CHANNEL, &OUTPUT_D1_STATE),
        OutputID::OutputD2 => (&OUTPUT_D2_CHANNEL, &OUTPUT_D2_STATE),
        OutputID::OutputD3 => (&OUTPUT_D3_CHANNEL, &OUTPUT_D3_STATE),
        OutputID::OutputD4 => (&OUTPUT_D4_CHANNEL, &OUTPUT_D4_STATE),
    };
    let sender = pin_state_watch.sender();

    // Initial state
    sender.send(pin_state(&pin));

    loop {
        // Wait for either a command or a pin edge
        match select::select(channel.recv_request(), pin.wait_for_any_edge()).await {
            // Handle command
            Either::First(command) => {
                match command {
                    Command::SetState(state) => {
                        pin.set_level(state.into());
                        sender.send(pin_state(&pin));
                        channel.send_response(()).await;
                    },
                }
            },
            
            // Handle pin edge
            Either::Second(_) => {
                sender.send(pin_state(&pin));
            },
        }
    }
}

/// Set the output state
/// 
/// # Arguments
/// * `output_id` - Which output to control
/// * `bool` - If false, pulls the output low. If true, lets it float.
pub async fn set_state(output_id: OutputID, state: bool) -> () {
    match output_id {
        OutputID::OutputD0 => OUTPUT_D0_CHANNEL.transact(Command::SetState(state)).await,
        OutputID::OutputD1 => OUTPUT_D1_CHANNEL.transact(Command::SetState(state)).await,
        OutputID::OutputD2 => OUTPUT_D2_CHANNEL.transact(Command::SetState(state)).await,
        OutputID::OutputD3 => OUTPUT_D3_CHANNEL.transact(Command::SetState(state)).await,
        OutputID::OutputD4 => OUTPUT_D4_CHANNEL.transact(Command::SetState(state)).await,
    }
}

/// Get a receiver that will be notified when the specified pin's state changes
pub fn watch_output(id: OutputID) -> Option<watch::Receiver<'static, CriticalSectionRawMutex, PinState, 4>> {
    match id {
        OutputID::OutputD0 => OUTPUT_D0_STATE.receiver(),
        OutputID::OutputD1 => OUTPUT_D1_STATE.receiver(),
        OutputID::OutputD2 => OUTPUT_D2_STATE.receiver(),
        OutputID::OutputD3 => OUTPUT_D3_STATE.receiver(),
        OutputID::OutputD4 => OUTPUT_D4_STATE.receiver(),
    }
}

/// Get the current state of a pin
/// Note: Use watch_pin_state() to get state changes instead of polling with this function
pub fn get_output_state(id: OutputID) -> Option<PinState> {
    match id {
        OutputID::OutputD0 => OUTPUT_D0_STATE.try_get(),
        OutputID::OutputD1 => OUTPUT_D1_STATE.try_get(),
        OutputID::OutputD2 => OUTPUT_D2_STATE.try_get(),
        OutputID::OutputD3 => OUTPUT_D3_STATE.try_get(),
        OutputID::OutputD4 => OUTPUT_D4_STATE.try_get(),
    }
}
