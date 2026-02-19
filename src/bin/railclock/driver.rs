use alloc::format;
use defmt::error;
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    semaphore::{GreedySemaphore, Semaphore},
};
use embassy_time::Timer;
use esp_hal::gpio::{Output, OutputConfig};
use mainboard::{
    board::{Motor0Pin, Motor1Pin},
    tasks::PowerHandle,
};
use mcp794xx::Timelike;
use rkyv::{rancor::Error, Archive, Deserialize, Serialize};

use crate::{rtc::RTC, CLOCK_DRIVER};

pub(crate) struct ClockDriver {
    semaphore: GreedySemaphore<CriticalSectionRawMutex>,
}

#[derive(Debug, Archive, Serialize, Deserialize, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub(crate) struct ClockDriverState {
    pin: u8,
    time: Option<i64>,
}

impl Default for ClockDriverState {
    fn default() -> Self {
        Self {
            pin: 0u8,
            time: None,
        }
    }
}

impl ClockDriverState {
    pub fn from(v: &ArchivedClockDriverState) -> Self {
        let time: Option<i64> = if v.time.is_some() {
            Some(v.time.unwrap().into())
        } else {
            None
        };

        Self { pin: v.pin, time }
    }
}

impl ClockDriver {
    pub fn new() -> Self {
        Self {
            semaphore: GreedySemaphore::new(0),
        }
    }

    pub fn push_forward(&self, n: usize) {
        self.semaphore.release(n);
    }

    pub async fn acquire(&self) -> usize {
        self.semaphore.acquire_all(1).await.unwrap().permits()
    }
}

async fn read_rtc_state() -> ClockDriverState {
    match RTC.read_nonvolatile(0u8, 64u8).await {
        Ok(data) => rkyv::access::<ArchivedClockDriverState, Error>(data.as_ref())
            .map(|i| ClockDriverState::from(i))
            .unwrap_or_default(),
        Err(e) => {
            error!(
                "Failed to read the rtc sram memory {}",
                format!("{:?}", e).as_str()
            );
            Default::default()
        }
    }
}

async fn write_rtc_state(state: &ClockDriverState) {
    let data = match rkyv::to_bytes::<Error>(state) {
        Ok(v) => v,
        Err(e) => {
            error!(
                "Failed to serialzie to json, {}",
                format!("{:?}", e).as_str()
            );
            return;
        }
    };

    match RTC.write_nonvolatile(0u8, data.as_ref()).await {
        Ok(()) => {}
        Err(e) => {
            error!(
                "Failed to write to rtc sram {}",
                format!("{:?}", e).as_str()
            );
        }
    }
}

#[embassy_executor::task]
async fn clock_task(motor_pin0: Motor0Pin, motor_pin1: Motor1Pin, power: PowerHandle) {
    let mut state = read_rtc_state().await;

    let pin0_state = (state.pin & 1) != 0;
    let pin1_state = ((state.pin & 2) >> 1) != 0;
    let mut pin0 = Output::new(motor_pin0, pin0_state.into(), OutputConfig::default());
    let mut pin1 = Output::new(motor_pin1, pin1_state.into(), OutputConfig::default());
    let driver = CLOCK_DRIVER.get().await;

    if let Some(last_update) = state.time.as_ref() {
        let current_time = RTC.get_datetime().await.unwrap_or_default().second() as i64;
        if current_time > *last_update {
            let diff = (current_time - last_update) / 60;
            driver.push_forward(diff as usize);
        }
    }

    loop {
        let n = driver.acquire().await;

        power.set_boost_converter(true).await;
        Timer::after_millis(100).await;

        for _ in 0..n {
            pin0.toggle();
            pin1.toggle();
            state.pin ^= 0x3;
            Timer::after_secs(1).await;
        }

        power.set_boost_converter(false).await;

        if let Ok(time) = RTC.get_datetime().await {
            state.time = Some(time.and_utc().timestamp());
            write_rtc_state(&state).await;
        }
    }
}

pub fn spawn_clock_task(
    spawner: &Spawner,
    motor_pin0: Motor0Pin,
    motor_pin1: Motor1Pin,
    power: PowerHandle,
) {
    spawner
        .spawn(clock_task(motor_pin0, motor_pin1, power))
        .expect("Failed to spawn clock task");
}
