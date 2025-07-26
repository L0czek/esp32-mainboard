use embedded_hal::i2c::I2c;
use thiserror::Error;

#[derive(Error)]
pub enum PowerControllerError<I2C: I2c> {
    #[error("I2C bus error")]
    I2cBusError(#[source] I2C::Error),

    #[error("I2C expander error")]
    I2CExpanderError(#[source] pcf857x::Error<I2C::Error>),
}

pub type Result<T, I2C: I2c> = core::result::Result<T, PowerControllerError<I2C>>;

mod controller;
