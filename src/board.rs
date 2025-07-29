
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_hal_bus::{i2c::AtomicDevice, util::AtomicCell};
use esp_hal::{
    i2c::master::{ConfigError, I2c},
    peripherals::*,
    Blocking,
};

use once_cell::sync::OnceCell;

use crate::tasks::{PowerRequest, PowerResponse};

#[allow(non_snake_case)]
pub struct Board {
    pub GlobalInt: GPIO7<'static>,
    pub BoostEn: GPIO15<'static>,

    pub A0: GPIO4<'static>,
    pub A1: GPIO5<'static>,
    pub A2: GPIO6<'static>,
    pub A3: GPIO0<'static>,
    pub A4: GPIO1<'static>,

    pub D0: GPIO23<'static>,
    pub D1: GPIO22<'static>,
    pub D2: GPIO21<'static>,
    pub D3: GPIO20<'static>,
    pub D4: GPIO19<'static>,

    pub Motor0: GPIO8<'static>,
    pub Motor1: GPIO18<'static>,

    pub U0Tx: GPIO16<'static>,
    pub U0Rx: GPIO17<'static>,

    pub Sda: GPIO10<'static>,
    pub Scl: GPIO11<'static>,

    pub BatVol: GPIO2<'static>,
    pub BoostVol: GPIO3<'static>,
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
    sda: GPIO10<'static>,
    scl: GPIO11<'static>,
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
        None => panic!("I2C bus accessed before initalization"),
    }
}

static CHARGER_CTL_REQ: OnceCell<Channel<CriticalSectionRawMutex, PowerRequest, 16>> =
    OnceCell::with_value(Channel::new());
static CHARGER_CTL_RESP: OnceCell<Channel<CriticalSectionRawMutex, PowerResponse, 16>> =
    OnceCell::with_value(Channel::new());

pub async fn send_power_controller_command(request: PowerRequest) {
    CHARGER_CTL_REQ.get().unwrap().send(request).await;
}

pub async fn recv_power_controller_command() -> PowerRequest {
    CHARGER_CTL_REQ.get().unwrap().receive().await
}

pub async fn send_power_controller_response(response: PowerResponse) {
    CHARGER_CTL_RESP.get().unwrap().send(response).await;
}

pub async fn recv_power_controller_response() -> PowerResponse {
    CHARGER_CTL_RESP.get().unwrap().receive().await
}

pub async fn transact_power_controller_command(request: PowerRequest) -> PowerResponse {
    send_power_controller_command(request).await;
    recv_power_controller_response().await
}
