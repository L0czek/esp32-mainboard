use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
/// This module implements the simple modules interface
use esp_hal::gpio::{DriveMode, Flex, Level, Output, OutputConfig, OutputPin};
use esp_hal::peripherals::*;
use once_cell::sync::OnceCell;

static OUTPUT_IO: OnceCell<Mutex<CriticalSectionRawMutex, OutputIO>> = OnceCell::new();

// App state to share with handlers
#[derive(Debug)]
struct OutputIO {
    d0: Flex<'static>,
    d1: Flex<'static>,
}

fn new_configured_output<Pin: OutputPin + 'static>(pin: Pin) -> Flex<'static> {
    let mut gpio = Output::new(
        pin,
        Level::High,
        OutputConfig::default().with_drive_mode(DriveMode::OpenDrain),
    )
    .into_flex();
    gpio.set_input_enable(true);
    gpio
}

impl OutputIO {
    fn new(d0: GPIO23<'static>, d1: GPIO22<'static>) -> Self {
        Self {
            d0: new_configured_output(d0),
            d1: new_configured_output(d1),
        }
    }
}

pub fn initialize_simple_output(d0: GPIO23<'static>, d1: GPIO22<'static>) -> () {
    OUTPUT_IO
        .set(Mutex::new(OutputIO::new(d0, d1)))
        .ok()
        .expect("Failed to initialize simple output");
}
pub enum OutputID {
    Output1,
    Output2,
}

pub async fn set_state(id: OutputID, state: bool) {
    let mut output_io = OUTPUT_IO
        .get()
        .expect("Simple output not initialized")
        .lock()
        .await;
    match id {
        OutputID::Output1 => output_io.d0.set_level(state.into()),
        OutputID::Output2 => output_io.d1.set_level(state.into()),
    }
}

#[derive(Debug)]
pub enum PinState {
    InLow,
    InHigh,
    PullingDown,
    FunckingBad,
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

fn pin_state(pin: &Flex<'static>) -> PinState {
    match (pin.is_high(), pin.is_set_low()) {
        (true, false) => PinState::InHigh,
        (false, false) => PinState::InLow,
        (false, true) => PinState::PullingDown,
        (true, true) => PinState::FunckingBad,
    }
}

pub async fn get_states() -> (PinState, PinState) {
    let output_io = OUTPUT_IO
        .get()
        .expect("Simple output not initialized")
        .lock()
        .await;
    (pin_state(&output_io.d0), pin_state(&output_io.d1))
}
