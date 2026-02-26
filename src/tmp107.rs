use defmt::info;
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::gpio::{Level, Output};
use esp_hal::uart::{UartRx, UartTx};
use esp_hal::Async;

// -- Protocol constants (VERIFY: datasheet docs/tmp107.pdf) --

/// Maximum sensors in a TMP107 daisy chain (5-bit address space).
pub const MAX_SENSORS: usize = 32;

/// Sent before every command so sensors can auto-detect baud rate.
const CALIBRATION_BYTE: u8 = 0x55;

/// Address Initialize command byte.
const ADDR_INIT_COMMAND: u8 = 0x95;

/// Temperature register address.
const TEMP_REGISTER: u8 = 0x00;

/// Timeout for a single sensor response (individual read).
const READ_TIMEOUT_MS: u64 = 10;

/// Timeout waiting for next address-init response before giving up.
const ADDR_DISCOVER_TIMEOUT_MS: u64 = 50;

/// Time for 3 TX bytes to physically leave the wire at 115200 baud
/// (3 * 10 bits / 115200 ≈ 260 µs). Rounded up for safety.
const TX_DRAIN_US: u64 = 500;

// -- Error type --

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum Tmp107Error {
    UartWrite,
    UartRead,
    Timeout,
    NoSensorsFound,
}

// -- Driver struct --

pub struct Tmp107 {
    tx: UartTx<'static, Async>,
    rx: UartRx<'static, Async>,
    dir: Output<'static>,
    sensor_count: u8,
}
