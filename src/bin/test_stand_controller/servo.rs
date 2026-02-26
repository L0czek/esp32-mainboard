use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::mcpwm::operator::PwmPinConfig;
use esp_hal::mcpwm::timer::PwmWorkingMode;
use esp_hal::mcpwm::{McPwm, PeripheralClockConfig};
use esp_hal::peripherals::MCPWM0;
use esp_hal::time::Rate;
use mainboard::board::D1Pin;

use crate::config::{
    SERVO_CLOSED_DEGREES, SERVO_FULL_RANGE_MS, SERVO_MAX_PULSE_TICKS,
    SERVO_MIN_PULSE_TICKS, SERVO_OPEN_DEGREES,
};
use crate::mqtt::commands::servo::ServoCommand;
use crate::mqtt::queue;
use crate::mqtt::sensors::slow::ServoSensorPacket;
use crate::mqtt::sensors::status::ServoStatus;

const TICK_INTERVAL_MS: u64 = 20;

static SERVO_COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, ServoCommand, 4> =
    Channel::new();

pub fn send_servo_command(command: ServoCommand) {
    if SERVO_COMMAND_CHANNEL.try_send(command).is_err() {
        warn!("Servo command channel full, dropping command");
    }
}

fn degrees_to_ticks(degrees: u16) -> u16 {
    let range = SERVO_MAX_PULSE_TICKS - SERVO_MIN_PULSE_TICKS;
    SERVO_MIN_PULSE_TICKS + ((degrees as u32 * range as u32) / 180) as u16
}

fn travel_time_ms(from_ticks: u16, to_ticks: u16) -> u64 {
    let tick_range = (SERVO_MAX_PULSE_TICKS - SERVO_MIN_PULSE_TICKS) as u64;
    let distance = from_ticks.abs_diff(to_ticks) as u64;
    SERVO_FULL_RANGE_MS * distance / tick_range
}

fn target_ticks_for_command(command: ServoCommand) -> u16 {
    match command {
        ServoCommand::Open => degrees_to_ticks(SERVO_OPEN_DEGREES),
        ServoCommand::Close => degrees_to_ticks(SERVO_CLOSED_DEGREES),
    }
}

fn status_for_command(command: ServoCommand) -> (ServoStatus, ServoStatus) {
    match command {
        ServoCommand::Open => (ServoStatus::Opening, ServoStatus::Open),
        ServoCommand::Close => (ServoStatus::Closing, ServoStatus::Closed),
    }
}

fn publish_servo_status(status: ServoStatus) {
    if queue::publish_servo_status(status).is_err() {
        warn!("Failed to publish servo status: queue full");
    }
}

fn publish_servo_position(ticks: u16) {
    let timestamp_ms = Instant::now().as_millis() as u32;
    let packet = ServoSensorPacket::new(timestamp_ms, ticks);
    if queue::publish_servo_sensor(packet).is_err() {
        warn!("Failed to publish servo position: queue full");
    }
}

#[embassy_executor::task]
pub async fn servo_controller_task(mcpwm: MCPWM0<'static>, pin: D1Pin) {
    let clock_cfg = PeripheralClockConfig::with_frequency(
        Rate::from_mhz(32),
    )
    .expect("Failed to configure MCPWM clock");

    let mut mcpwm = McPwm::new(mcpwm, clock_cfg);
    mcpwm.operator0.set_timer(&mcpwm.timer0);

    let mut pwm_pin = mcpwm
        .operator0
        .with_pin_a(pin, PwmPinConfig::UP_ACTIVE_HIGH);

    let timer_clock_cfg = clock_cfg
        .timer_clock_with_frequency(
            19_999,
            PwmWorkingMode::Increase,
            Rate::from_hz(50),
        )
        .expect("Failed to configure MCPWM timer");
    mcpwm.timer0.start(timer_clock_cfg);

    // Boot: drive to closed position
    let closed_ticks = degrees_to_ticks(SERVO_CLOSED_DEGREES);
    let mut current_ticks = closed_ticks;
    pwm_pin.set_timestamp(current_ticks);
    publish_servo_status(ServoStatus::Closed);
    publish_servo_position(current_ticks);
    info!(
        "Servo initialized at closed position ({} ticks)",
        current_ticks
    );

    loop {
        let command = SERVO_COMMAND_CHANNEL.receive().await;
        let target_ticks = target_ticks_for_command(command);

        if target_ticks == current_ticks {
            continue;
        }

        let (moving_status, arrived_status) = status_for_command(command);
        publish_servo_status(moving_status);

        let total_time_ms = travel_time_ms(current_ticks, target_ticks);
        let total_steps = total_time_ms / TICK_INTERVAL_MS;
        let start_ticks = current_ticks;

        if total_steps == 0 {
            current_ticks = target_ticks;
            pwm_pin.set_timestamp(current_ticks);
            publish_servo_position(current_ticks);
            publish_servo_status(arrived_status);
            continue;
        }

        let mut step: u64 = 0;
        let mut reached = false;

        while !reached {
            match select(
                SERVO_COMMAND_CHANNEL.receive(),
                Timer::after(Duration::from_millis(TICK_INTERVAL_MS)),
            )
            .await
            {
                Either::First(new_command) => {
                    let new_target = target_ticks_for_command(new_command);
                    if new_target == current_ticks {
                        let (_, new_arrived) =
                            status_for_command(new_command);
                        publish_servo_status(new_arrived);
                        reached = true;
                        continue;
                    }
                    if new_target == target_ticks {
                        continue;
                    }
                    send_servo_command(new_command);
                    reached = true;
                    continue;
                }
                Either::Second(()) => {
                    step += 1;
                    if step >= total_steps {
                        current_ticks = target_ticks;
                        reached = true;
                    } else {
                        let progress = step as i32;
                        let total = total_steps as i32;
                        let delta = (target_ticks as i32
                            - start_ticks as i32)
                            * progress
                            / total;
                        let raw = start_ticks as i32 + delta;
                        let clamped = raw.clamp(
                            SERVO_MIN_PULSE_TICKS as i32,
                            SERVO_MAX_PULSE_TICKS as i32,
                        );
                        current_ticks = clamped as u16;
                    }
                    pwm_pin.set_timestamp(current_ticks);
                    publish_servo_position(current_ticks);
                }
            }
        }

        if current_ticks == target_ticks {
            publish_servo_status(arrived_status);
        }
    }
}
