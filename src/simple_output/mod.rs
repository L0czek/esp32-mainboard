use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
/// This module implements the simple modules interface
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::peripherals::*;
use once_cell::sync::OnceCell;

static OUTPUT_IO: OnceCell<Mutex<CriticalSectionRawMutex, OutputIO>> = OnceCell::new();

// App state to share with handlers
#[derive(Debug)]
struct OutputIO {
    d0: Output<'static>,
    d1: Output<'static>,
}

impl OutputIO {
    fn new(d0: GPIO23<'static>, d1: GPIO22<'static>) -> Self {
        Self {
            d0: Output::new(d0, Level::Low, OutputConfig::default()),
            d1: Output::new(d1, Level::Low, OutputConfig::default()),
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
