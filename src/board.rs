
use embedded_hal_bus::{i2c::AtomicDevice, util::AtomicCell};
use esp_hal::{
    i2c::master::{ConfigError, I2c},
    peripherals::*,
    Blocking,
};

use once_cell::sync::OnceCell;

use crate::{channel::RequestResponseChannel, tasks::{PowerRequest, PowerResponse}};

pub type GlobalIntPin = GPIO7<'static>;
pub type BoostEnPin = GPIO15<'static>;

pub type A0Pin = GPIO4<'static>;
pub type A1Pin = GPIO5<'static>;
pub type A2Pin = GPIO6<'static>;
pub type A3Pin = GPIO0<'static>;
pub type A4Pin = GPIO1<'static>;

pub type D0Pin = GPIO23<'static>;
pub type D1Pin = GPIO22<'static>;
pub type D2Pin = GPIO21<'static>;
pub type D3Pin = GPIO20<'static>;
pub type D4Pin = GPIO19<'static>;

pub type Motor0Pin = GPIO8<'static>;
pub type Motor1Pin = GPIO18<'static>;

pub type U0TxPin = GPIO16<'static>;
pub type U0RxPin = GPIO17<'static>;

pub type SdaPin = GPIO10<'static>;
pub type SclPin = GPIO11<'static>;

pub type BatVolPin = GPIO2<'static>;
pub type BoostVolPin = GPIO3<'static>;

#[allow(non_snake_case)]
pub struct Board {
    pub GlobalInt: GlobalIntPin,
    pub BoostEn: BoostEnPin,

    pub A0: A0Pin,
    pub A1: A1Pin,
    pub A2: A2Pin,
    pub A3: A3Pin,
    pub A4: A4Pin,

    pub D0: D0Pin,
    pub D1: D1Pin,
    pub D2: D2Pin,
    pub D3: D3Pin,
    pub D4: D4Pin,

    pub Motor0: Motor0Pin,
    pub Motor1: Motor1Pin,

    pub U0Tx: U0TxPin,
    pub U0Rx: U0RxPin,

    pub Sda: SdaPin,
    pub Scl: SclPin,

    pub BatVol: BatVolPin,
    pub BoostVol: BoostVolPin,
}

#[macro_export]
macro_rules! create_board {
    ($peripherals:expr) => {
        Board {
            GlobalInt: $peripherals.GPIO7,
            BoostEn: $peripherals.GPIO15,

            A0: $peripherals.GPIO4,
            A1: $peripherals.GPIO5,
            A2: $peripherals.GPIO6,
            A3: $peripherals.GPIO0,
            A4: $peripherals.GPIO1,

            D0: $peripherals.GPIO23,
            D1: $peripherals.GPIO22,
            D2: $peripherals.GPIO21,
            D3: $peripherals.GPIO20,
            D4: $peripherals.GPIO19,

            Motor0: $peripherals.GPIO8,
            Motor1: $peripherals.GPIO18,

            U0Tx: $peripherals.GPIO16,
            U0Rx: $peripherals.GPIO17,

            Sda: $peripherals.GPIO10,
            Scl: $peripherals.GPIO11,

            BatVol: $peripherals.GPIO2,
            BoostVol: $peripherals.GPIO3,
        }
    };
}

static I2C_BUS: OnceCell<AtomicCell<I2c<'static, Blocking>>> = OnceCell::new();

pub fn init_i2c_bus(
    i2c0: I2C0<'static>,
    sda: SdaPin,
    scl: SclPin,
) -> Result<(), ConfigError> {
    let bus = I2c::new(i2c0, Default::default())?
        .with_sda(sda)
        .with_scl(scl);

    let _ = I2C_BUS.set(AtomicCell::new(bus));

    Ok(())
}

pub fn acquire_i2c_bus() -> AtomicDevice<'static, I2c<'static, Blocking>> {
    match I2C_BUS.get() {
        Some(bus) => AtomicDevice::new(bus),
        None => panic!("I2C bus accessed before initialization"),
    }
}

pub static POWER_CONTROL: RequestResponseChannel<PowerRequest, PowerResponse, 16> = RequestResponseChannel::with_static_channels();
