use crate::{
    config::{
        SERVO_CLOSED_DEGREES, SERVO_MAX_PULSE_TICKS, SERVO_MIN_PULSE_TICKS, SERVO_OPEN_DEGREES,
    },
    servo::state::ServoStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ServoCommand {
    Open,
    Close,
}

fn degrees_to_ticks(degrees: u16) -> u16 {
    let range = SERVO_MAX_PULSE_TICKS - SERVO_MIN_PULSE_TICKS;
    SERVO_MIN_PULSE_TICKS + ((degrees as u32 * range as u32) / 1800) as u16
}

impl ServoCommand {
    pub(super) fn target_ticks(self) -> u16 {
        match self {
            ServoCommand::Open => degrees_to_ticks(SERVO_OPEN_DEGREES),
            ServoCommand::Close => degrees_to_ticks(SERVO_CLOSED_DEGREES),
        }
    }

    // returns (moving status, reached status)
    pub(super) fn status(self) -> (ServoStatus, ServoStatus) {
        match self {
            ServoCommand::Open => (ServoStatus::Opening, ServoStatus::Open),
            ServoCommand::Close => (ServoStatus::Closing, ServoStatus::Closed),
        }
    }
}
