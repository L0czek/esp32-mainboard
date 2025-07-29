use core::fmt::Display;

use defmt::Format;

use crate::{power::PowerControllerError, I2cType};

#[derive(Format)]
pub enum AnyError {
    PowerControllerError(PowerControllerError<I2cType>),
}

impl Display for AnyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AnyError::PowerControllerError(err) => write!(f, "{}", err),
        }
    }
}

impl From<PowerControllerError<I2cType>> for AnyError {
    fn from(value: PowerControllerError<I2cType>) -> Self {
        Self::PowerControllerError(value)
    }
}
