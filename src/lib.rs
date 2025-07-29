#![no_std]

pub mod board;
pub mod error;
pub mod power;
pub mod tasks;

use embedded_hal_bus::i2c::AtomicDevice;
use error::AnyError;
use esp_hal::{i2c::master::I2c, Blocking};

pub type I2cType = AtomicDevice<'static, I2c<'static, Blocking>>;
pub type Result<T> = core::result::Result<T, AnyError>;
