use core::sync::atomic::{AtomicU8, Ordering};

use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::Input;
use mainboard::board::I2cType;
use mainboard::fire_trigger::FireTrigger;
use mainboard::signal_light::{SignalLight, SignalLightConfig};

const FIRE_TRIGGER_BYTE: u8 = 0x00;

use crate::camera_shutter;
use crate::mqtt::commands::state::StateCommand;
use crate::mqtt::queue;
use crate::mqtt::sensors::digital::ArmedPacket;
use crate::mqtt::sensors::status::StateStatus;

const FIRE_BUZZER_DURATION_MS: u64 = 3000;

enum SequencerMessage {
    Command(StateCommand),
    BuzzerComplete,
}

static SEQUENCER_CHANNEL: Channel<CriticalSectionRawMutex, SequencerMessage, 4> = Channel::new();

static FIRE_ACTIVATE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static FIRE_CANCEL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

static LAST_ARMED_VALUE: AtomicU8 = AtomicU8::new(0);
static CURRENT_STATE: AtomicU8 = AtomicU8::new(0);

pub fn send_state_command(command: StateCommand) {
    let msg = SequencerMessage::Command(command);
    if SEQUENCER_CHANNEL.try_send(msg).is_err() {
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
        1 => StateStatus::Fire,
        2 => StateStatus::PostFire,
        _ => StateStatus::Armed,
    }
}

pub fn republish_sequencer_state() {
    let status = load_state();
    let _ = queue::publish_state_status(status);
}

pub fn republish_armed_state() {
    let value = LAST_ARMED_VALUE.load(Ordering::Relaxed);
    let packet = ArmedPacket::new(timestamp_ms(), value);
    let _ = crate::mqtt::publish_armed_sensor(packet);
}

fn store_state(status: StateStatus) {
    let v = match status {
        StateStatus::Armed => 0,
        StateStatus::Fire => 1,
        StateStatus::PostFire => 2,
    };
    CURRENT_STATE.store(v, Ordering::Relaxed);
}

fn timestamp_ms() -> u32 {
    Instant::now().as_millis() as u32
}

fn is_safety_armed(pin: &Input<'_>) -> bool {
    pin.is_high()
}

fn publish_armed_change(pin: &Input<'_>) {
    let value = pin.is_high() as u8;
    LAST_ARMED_VALUE.store(value, Ordering::Relaxed);
    let packet = ArmedPacket::new(timestamp_ms(), value);

    info!("Armed switch: {}", value);
    if crate::mqtt::publish_armed_sensor(packet).is_err() {
        warn!("Dropping armed packet: outbound queue full");
    }
}

fn transition_state(state: &mut StateStatus, new_state: StateStatus) {
    *state = new_state;
    store_state(new_state);
    let _ = queue::publish_state_status(new_state);
    queue::publish_command_log(new_state.as_log());
    info!("State: {}", new_state.as_str());
}

fn set_light(light: &mut SignalLight<I2cType>, config: SignalLightConfig) {
    if let Err(_e) = light.set(config) {
        warn!("Failed to set signal light");
    }
}

#[embassy_executor::task]
pub async fn fire_sequencer_task(fire_trigger_i2c: I2cType) {
    let address = pcf857x::SlaveAddr::Alternative(false, false, false);
    let mut trigger = match FireTrigger::new(fire_trigger_i2c, address, FIRE_TRIGGER_BYTE) {
        Ok(t) => t,
        Err(_e) => {
            warn!("Failed to initialize fire trigger");
            return;
        }
    };

    loop {
        FIRE_ACTIVATE.wait().await;
        FIRE_CANCEL.reset();
        match select(
            Timer::after(Duration::from_millis(FIRE_BUZZER_DURATION_MS)),
            FIRE_CANCEL.wait(),
        )
        .await
        {
            Either::First(()) => {
                if let Err(_e) = trigger.trigger() {
                    warn!("Failed to activate fire trigger");
                }
                let msg = SequencerMessage::BuzzerComplete;
                if SEQUENCER_CHANNEL.try_send(msg).is_err() {
                    warn!("Failed to send buzzer complete");
                }
            }
            Either::Second(()) => {
                if let Err(_e) = trigger.abort() {
                    warn!("Failed to abort fire trigger");
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn state_sequencer_task(mut armed_pin: Input<'static>, signal_light_i2c: I2cType) {
    let address = pcf857x::SlaveAddr::Alternative(false, false, true);
    let mut light = match SignalLight::new(signal_light_i2c, address) {
        Ok(light) => light,
        Err(_e) => {
            warn!("Failed to initialize signal light");
            return;
        }
    };

    let mut state = StateStatus::Armed;
    store_state(state);
    set_light(
        &mut light,
        SignalLightConfig {
            green: true,
            ..SignalLightConfig::default()
        },
    );
    info!("State sequencer initialized: ARMED");

    loop {
        match select(SEQUENCER_CHANNEL.receive(), armed_pin.wait_for_any_edge()).await {
            Either::First(msg) => match msg {
                SequencerMessage::Command(cmd) => {
                    handle_command(cmd, &mut state, &armed_pin, &mut light);
                }
                SequencerMessage::BuzzerComplete => {
                    if state == StateStatus::Fire {
                        set_light(
                            &mut light,
                            SignalLightConfig {
                                red: true,
                                ..SignalLightConfig::default()
                            },
                        );
                    }
                }
            },
            Either::Second(()) => {
                publish_armed_change(&armed_pin);
            }
        }
    }
}

fn handle_command(
    command: StateCommand,
    state: &mut StateStatus,
    armed_pin: &Input<'_>,
    light: &mut SignalLight<I2cType>,
) {
    match command {
        StateCommand::Fire => {
            if *state != StateStatus::Armed {
                warn!("FIRE rejected: not in ARMED state");
                queue::publish_command_log("FIRE rejected: not in ARMED state");
                return;
            }
            if !is_safety_armed(armed_pin) {
                warn!("FIRE rejected: safety switch not armed");
                queue::publish_command_log("FIRE rejected: safety switch not armed");
                return;
            }

            transition_state(state, StateStatus::Fire);
            set_light(
                light,
                SignalLightConfig {
                    red: true,
                    buzzer: true,
                    ..SignalLightConfig::default()
                },
            );
            camera_shutter::trigger_shutter();
            FIRE_ACTIVATE.signal(());
        }
        StateCommand::FireEnd => {
            if *state != StateStatus::Fire {
                warn!("FIRE_END rejected: not in FIRE state");
                queue::publish_command_log("FIRE_END rejected: not in FIRE state");
                return;
            }
            FIRE_CANCEL.signal(());
            camera_shutter::trigger_shutter();
            transition_state(state, StateStatus::PostFire);
            set_light(
                light,
                SignalLightConfig {
                    green: true,
                    red: true,
                    ..SignalLightConfig::default()
                },
            );
        }
        StateCommand::FireReset => {
            if *state != StateStatus::PostFire {
                warn!("FIRE_RESET rejected: not in POSTFIRE state");
                queue::publish_command_log("FIRE_RESET rejected: not in POSTFIRE state");
                return;
            }
            transition_state(state, StateStatus::Armed);
            set_light(
                light,
                SignalLightConfig {
                    green: true,
                    ..SignalLightConfig::default()
                },
            );
        }
    }
}
