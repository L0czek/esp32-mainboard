pub mod servo;
pub mod shutdown;
pub mod state;

use defmt::{info, warn};

use crate::mqtt::commands::servo::ServoCommand;
use crate::mqtt::commands::shutdown::ShutdownCommand;
use crate::mqtt::commands::state::StateCommand;
use crate::mqtt::sensors::status::StateStatus;
use crate::mqtt::topics::{TOPIC_CMD_SERVO, TOPIC_CMD_SHUTDOWN, TOPIC_CMD_STATE};

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum CommandError {
    UnknownTopic,
    InvalidPayload,
}

pub trait StateCommandHandler {
    fn handle_state_command(&mut self, command: StateCommand);
}

pub trait ServoCommandHandler {
    fn handle_servo_command(&mut self, command: ServoCommand);
}

pub trait ShutdownCommandHandler {
    fn handle_shutdown_command(&mut self, command: ShutdownCommand);
}

pub struct CommandDispatcher<H: StateCommandHandler + ServoCommandHandler + ShutdownCommandHandler>
{
    handlers: H,
}

impl<H: StateCommandHandler + ServoCommandHandler + ShutdownCommandHandler> CommandDispatcher<H> {
    pub const fn new(handlers: H) -> Self {
        Self { handlers }
    }

    pub fn dispatch(&mut self, topic: &str, payload: &[u8]) -> Result<(), CommandError> {
        if topic == TOPIC_CMD_STATE {
            let command = StateCommand::decode(payload).ok_or(CommandError::InvalidPayload)?;
            self.handlers.handle_state_command(command);
            return Ok(());
        }

        if topic == TOPIC_CMD_SERVO {
            let command = ServoCommand::decode(payload).ok_or(CommandError::InvalidPayload)?;
            self.handlers.handle_servo_command(command);
            return Ok(());
        }

        if topic == TOPIC_CMD_SHUTDOWN {
            let command = ShutdownCommand::decode(payload).ok_or(CommandError::InvalidPayload)?;
            self.handlers.handle_shutdown_command(command);
            return Ok(());
        }

        Err(CommandError::UnknownTopic)
    }
}

pub struct MockCommandHandlers {
    state: StateStatus,
}

impl MockCommandHandlers {
    pub const fn new() -> Self {
        Self {
            state: StateStatus::Armed,
        }
    }
}

impl StateCommandHandler for MockCommandHandlers {
    fn handle_state_command(&mut self, command: StateCommand) {
        match command {
            StateCommand::Fire => {
                self.state = StateStatus::Fire;
                info!("MQTT command: FIRE");
            }
            StateCommand::FireEnd => {
                self.state = StateStatus::PostFire;
                info!("MQTT command: FIRE_END");
            }
            StateCommand::FireReset => {
                self.state = StateStatus::Armed;
                info!("MQTT command: FIRE_RESET");
            }
        }
    }
}

impl ServoCommandHandler for MockCommandHandlers {
    fn handle_servo_command(&mut self, command: ServoCommand) {
        if self.state == StateStatus::Fire {
            warn!("MQTT command ignored: cmd/servo in FIRE state");
            return;
        }

        match command {
            ServoCommand::Open => info!("MQTT command: OPEN"),
            ServoCommand::Close => info!("MQTT command: CLOSE"),
        }
    }
}

impl ShutdownCommandHandler for MockCommandHandlers {
    fn handle_shutdown_command(&mut self, command: ShutdownCommand) {
        match command {
            ShutdownCommand::Shutdown => info!("MQTT command: SHUTDOWN"),
        }
    }
}
