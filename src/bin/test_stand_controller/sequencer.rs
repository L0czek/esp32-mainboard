use core::sync::atomic::{AtomicU8, Ordering};

use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::Input;
use mainboard::board::I2cType;
use mainboard::fire_trigger::FireTrigger;
use mainboard::signal_light::{SignalLight, SignalLightConfig};

use crate::camera_shutter::{self, CameraShutterCommand};
use crate::config::{FIRE_COUNTDOWN_DURATION_MS, FIRE_TRIGGER_BYTE};
use crate::mqtt::commands::state::StateCommand;
use crate::mqtt::queue;
use crate::mqtt::sensors::digital::ArmedPacket;
use crate::mqtt::sensors::status::StateStatus;
use crate::mqtt::sensors::EncodableEnum;

static SEQUENCER_CHANNEL: Channel<CriticalSectionRawMutex, StateCommand, 4> = Channel::new();

static LAST_ARMED_VALUE: AtomicU8 = AtomicU8::new(0);
static CURRENT_STATE: AtomicU8 = AtomicU8::new(0);

pub fn send_state_command(command: StateCommand) {
    if SEQUENCER_CHANNEL.try_send(command).is_err() {
        warn!("Sequencer command channel full, dropping command");
    }
}

pub fn init_armed_state(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    info!("Armed switch initial state: {}", value);
}

pub fn load_state() -> StateStatus {
    match CURRENT_STATE.load(Ordering::Relaxed) {
        1 => StateStatus::Countdown {
            end: Instant::now(), // Approximation, real deadline not stored. This is not actually send
        },
        2 => StateStatus::Fire,
        3 => StateStatus::PostFire,
        _ => StateStatus::Armed,
    }
}

pub fn republish_sequencer_state() {
    let status = load_state();
    queue::publish_state_status(status);
}

pub fn republish_armed_state() {
    let value = LAST_ARMED_VALUE.load(Ordering::Relaxed);
    let ts = now_ms();
    let packet = ArmedPacket::new(ts, value);
    crate::mqtt::publish_armed_sensor(packet);
    crate::blackbox::send_to_blackbox(crate::blackbox::BlackboxPacket::Digital { value });
}

fn store_state(status: StateStatus) {
    let v = match status {
        StateStatus::Armed => 0,
        StateStatus::Countdown { .. } => 1,
        StateStatus::Fire => 2,
        StateStatus::PostFire => 3,
    };
    CURRENT_STATE.store(v, Ordering::Relaxed);
}

fn now_ms() -> u32 {
    Instant::now().as_millis() as u32
}

fn publish_armed_change(value: u8) {
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    let packet = ArmedPacket::new(now_ms(), value);

    info!("Armed switch: {}", value);
    crate::mqtt::publish_armed_sensor(packet);
    crate::blackbox::send_to_blackbox(crate::blackbox::BlackboxPacket::Digital { value });
}

fn armed_value() -> u8 {
    LAST_ARMED_VALUE.load(Ordering::Relaxed)
}

fn is_safety_armed() -> bool {
    armed_value() != 0
}

#[embassy_executor::task]
pub async fn armed_pin_task(mut armed_pin: Input<'static>) {
    let mut last_value = armed_pin.is_high() as u8;
    publish_armed_change(last_value);
    loop {
        if last_value == 0 {
            armed_pin.wait_for_high().await;
            last_value = 1;
        } else {
            armed_pin.wait_for_low().await;
            last_value = 0;
            send_state_command(StateCommand::SafetySafe);
        }
        publish_armed_change(last_value);
    }
}

struct Sequencer {
    state: StateStatus,
    light: SignalLight<I2cType>,
    fire_trigger: FireTrigger<I2cType>,
}

impl Sequencer {
    fn new(signal_light_i2c: I2cType, fire_trigger_i2c: I2cType) -> Option<Self> {
        let light_address = pcf857x::SlaveAddr::Alternative(false, false, true);
        let mut light = match SignalLight::new(signal_light_i2c, light_address) {
            Ok(light) => light,
            Err(_e) => {
                warn!("Failed to initialize signal light");
                return None;
            }
        };

        let fire_address = pcf857x::SlaveAddr::Alternative(false, false, false);
        let fire_trigger = match FireTrigger::new(fire_trigger_i2c, fire_address, FIRE_TRIGGER_BYTE)
        {
            Ok(trigger) => trigger,
            Err(_e) => {
                warn!("Failed to initialize fire trigger");
                return None;
            }
        };

        let state = StateStatus::Armed;
        store_state(state);

        if let Err(_e) = light.set(SignalLightConfig {
            green: true,
            ..SignalLightConfig::default()
        }) {
            warn!("Failed to set initial signal light state");
        }

        info!("State sequencer initialized: ARMED");
        Some(Self {
            state,
            light,
            fire_trigger,
        })
    }

    fn transition_state(&mut self, new_state: StateStatus) {
        self.state = new_state;
        store_state(new_state);
        queue::publish_state_status(new_state);
        queue::publish_command_log(new_state.as_log());
        info!("State: {}", new_state.as_str());

        // set lights for state
        let new_light_config = match new_state {
            StateStatus::Armed => SignalLightConfig {
                green: true,
                ..SignalLightConfig::default()
            },
            StateStatus::Countdown { .. } => SignalLightConfig {
                red: true,
                buzzer: true,
                ..SignalLightConfig::default()
            },
            StateStatus::Fire => SignalLightConfig {
                red: true,
                ..SignalLightConfig::default()
            },
            StateStatus::PostFire => SignalLightConfig {
                green: true,
                red: true,
                ..SignalLightConfig::default()
            },
        };

        // set camera
        match new_state {
            StateStatus::Armed => {} // Already stopped at postfire
            StateStatus::Countdown { .. } => {
                camera_shutter::send_camera_command(CameraShutterCommand::Record)
            }
            StateStatus::Fire => {} // Already triggered at countdown end
            StateStatus::PostFire => {
                camera_shutter::send_camera_command(CameraShutterCommand::Stop)
            }
        }

        // trigger
        match new_state {
            StateStatus::Armed | StateStatus::Countdown { .. } => {} // No trigger action
            StateStatus::Fire => {
                if let Err(_e) = self.fire_trigger.trigger() {
                    warn!("Failed to activate fire trigger");
                }
            }
            StateStatus::PostFire => {
                if let Err(_e) = self.fire_trigger.safe() {
                    warn!("Failed to reset fire trigger");
                }
            }
        }

        if let Err(_e) = self.light.set(new_light_config) {
            warn!(
                "Failed to set signal light for state {}",
                new_state.as_str()
            );
        }
    }
}

#[embassy_executor::task]
pub async fn state_sequencer_task(signal_light_i2c: I2cType, fire_trigger_i2c: I2cType) {
    let Some(mut sequencer) = Sequencer::new(signal_light_i2c, fire_trigger_i2c) else {
        warn!("Failed to initialize state sequencer");
        return;
    };

    loop {
        match sequencer.state {
            StateStatus::Armed | StateStatus::Fire | StateStatus::PostFire => {
                let command = SEQUENCER_CHANNEL.receive().await;
                handle_command(command, &mut sequencer);
            }
            StateStatus::Countdown { end } => {
                match select(SEQUENCER_CHANNEL.receive(), Timer::at(end)).await {
                    Either::First(command) => {
                        handle_command(command, &mut sequencer);
                    }
                    Either::Second(()) => {
                        // Safety extra check
                        if !is_safety_armed() {
                            warn!("Countdown aborted: safety switch not armed");
                            queue::publish_command_log(
                                "Countdown aborted: safety switch not armed",
                            );
                            sequencer.transition_state(StateStatus::PostFire);

                            continue;
                        }

                        sequencer.transition_state(StateStatus::Fire);
                    }
                }
            }
        }
    }
}

fn handle_command(command: StateCommand, sequencer: &mut Sequencer) {
    match command {
        StateCommand::Fire => {
            if sequencer.state != StateStatus::Armed {
                warn!("FIRE rejected: not in ARMED state");
                queue::publish_command_log("FIRE rejected: not in ARMED state");
                return;
            }
            if !is_safety_armed() {
                warn!("FIRE rejected: safety switch not armed");
                queue::publish_command_log("FIRE rejected: safety switch not armed");
                return;
            }

            sequencer.transition_state(StateStatus::Countdown {
                end: Instant::now() + Duration::from_millis(FIRE_COUNTDOWN_DURATION_MS),
            });
        }
        StateCommand::SafetySafe => {
            if let StateStatus::Countdown { .. } = sequencer.state {
                info!("Safety switch triggered: aborting countdown");
                queue::publish_command_log("Safety switch triggered: aborting countdown");
                sequencer.transition_state(StateStatus::PostFire);
            }
        }
        StateCommand::Abort => {
            if let StateStatus::Countdown { .. } = sequencer.state {
                sequencer.transition_state(StateStatus::PostFire);
            } else {
                warn!("ABORT rejected: not in COUNTDOWN state");
                queue::publish_command_log("ABORT rejected: not in COUNTDOWN state");
            }
        }
        StateCommand::FireEnd => {
            if sequencer.state != StateStatus::Fire {
                warn!("FIRE_END rejected: not in FIRE state");
                queue::publish_command_log("FIRE_END rejected: not in FIRE state");
                return;
            }
            sequencer.transition_state(StateStatus::PostFire);
        }
        StateCommand::FireReset => {
            if sequencer.state != StateStatus::PostFire {
                warn!("FIRE_RESET rejected: not in POSTFIRE state");
                queue::publish_command_log("FIRE_RESET rejected: not in POSTFIRE state");
                return;
            }
            sequencer.transition_state(StateStatus::Armed);
        }
    }
}
