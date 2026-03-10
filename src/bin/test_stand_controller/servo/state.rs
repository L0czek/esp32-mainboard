use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

use defmt::error;

static CURRENT_SERVO_STATUS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ServoStatus {
    Closed,
    Opening,
    Open,
    Closing,
}

impl ServoStatus {
    pub const fn as_log(self) -> &'static str {
        match self {
            Self::Closed => "Servo closed",
            Self::Opening => "Servo opening",
            Self::Open => "Servo open",
            Self::Closing => "Servo closing",
        }
    }

    pub(super) fn store(self) {
        let encoded = match self {
            ServoStatus::Closed => 0,
            ServoStatus::Opening => 1,
            ServoStatus::Open => 2,
            ServoStatus::Closing => 3,
        };
        CURRENT_SERVO_STATUS.store(encoded, Ordering::Relaxed);
    }

    pub(super) fn load() -> Self {
        match CURRENT_SERVO_STATUS.load(Ordering::Relaxed) {
            0 => ServoStatus::Closed,
            1 => ServoStatus::Opening,
            2 => ServoStatus::Open,
            3 => ServoStatus::Closing,
            // below state is impossible. Log and return something
            state => {
                error!("Impossible servo state: {}", state);
                ServoStatus::Opening
            }
        }
    }
}

static CURRENT_SERVO_TICKS: AtomicU16 = AtomicU16::new(0);

pub(super) fn current_servo_ticks() -> u16 {
    CURRENT_SERVO_TICKS.load(Ordering::Relaxed)
}

pub(super) fn store_servo_ticks(ticks: u16) {
    CURRENT_SERVO_TICKS.store(ticks, Ordering::Relaxed);
}
