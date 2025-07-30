use core::fmt::Display;

use defmt::{write as defmt_write, Format};
use embedded_hal::i2c::I2c;

#[derive(Debug)]
pub enum PowerControllerError<I2C: I2c> {
    I2cBusError(I2C::Error),
    I2CExpanderError(pcf857x::Error<I2C::Error>),
}

impl<I2C: I2c> Display for PowerControllerError<I2C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PowerControllerError::I2cBusError(bus_error) => write!(
                f,
                "Power Controller error due to I2C bus malfunction: {:?}",
                bus_error
            ),
            PowerControllerError::I2CExpanderError(expander_err) => write!(
                f,
                "Power Controller error due to I2C expander error {:?}",
                expander_err
            ),
        }
    }
}

impl<I2C: I2c> Format for PowerControllerError<I2C> {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            PowerControllerError::I2cBusError(bus_error) => {
                defmt_write!(fmt, "Power Controller error due to I2C bus malfunction")
            }
            PowerControllerError::I2CExpanderError(expander_err) => {
                defmt_write!(fmt, "Power Controller error due to I2C expander error")
            }
        }
    }
}

pub type Result<T, I2C> = core::result::Result<T, PowerControllerError<I2C>>;

mod controller;

pub use controller::{
    PowerController, PowerControllerConfig, PowerControllerIO, PowerControllerMode,
    PowerControllerStats,
};
