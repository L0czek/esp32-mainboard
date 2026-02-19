use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, TrySendError};

use crate::mqtt::sensors::fast::{FastAdcChannel, FastAdcPacket};
use crate::mqtt::sensors::slow::{ArmedPacket, ServoSensorPacket, SlowAdcChannel, SlowAdcPacket};
use crate::mqtt::sensors::status::{CommandStatusPacket, ServoStatus, StateStatus};
use crate::mqtt::sensors::temp::TempPacket;

pub const OUTBOUND_QUEUE_CAPACITY: usize = 128;

static OUTBOUND_QUEUE: Channel<CriticalSectionRawMutex, OutboundMessage, OUTBOUND_QUEUE_CAPACITY> =
    Channel::new();

#[derive(Debug, Clone)]
pub enum OutboundMessage {
    FastAdc(FastAdcPacket),
    SlowAdc(SlowAdcPacket),
    Armed(ArmedPacket),
    Temp(TempPacket),
    ServoSensor(ServoSensorPacket),
    StateStatus(StateStatus),
    ServoStatus(ServoStatus),
    CommandStatus(CommandStatusPacket),
}

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum PublishError {
    QueueFull,
}

#[derive(Debug, Clone)]
pub struct FastSensorsBatch {
    pub tensometer: Option<FastAdcPacket>,
    pub tank_pressure: Option<FastAdcPacket>,
    pub combustion_pressure: Option<FastAdcPacket>,
}

impl FastSensorsBatch {
    pub const fn empty() -> Self {
        Self {
            tensometer: None,
            tank_pressure: None,
            combustion_pressure: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlowSensorsBatch {
    pub battery_stand: Option<SlowAdcPacket>,
    pub battery_computer: Option<SlowAdcPacket>,
    pub boost_voltage: Option<SlowAdcPacket>,
    pub starter_sense: Option<SlowAdcPacket>,
    pub servo: Option<ServoSensorPacket>,
}

impl SlowSensorsBatch {
    pub fn empty() -> Self {
        Self {
            battery_stand: None,
            battery_computer: None,
            boost_voltage: None,
            starter_sense: None,
            servo: None,
        }
    }
}

pub fn publish_fast_sensors(batch: FastSensorsBatch) -> Result<(), PublishError> {
    if let Some(mut packet) = batch.tensometer {
        packet.set_channel(FastAdcChannel::Tensometer);
        enqueue(OutboundMessage::FastAdc(packet))?;
    }

    if let Some(mut packet) = batch.tank_pressure {
        packet.set_channel(FastAdcChannel::PressureTank);
        enqueue(OutboundMessage::FastAdc(packet))?;
    }

    if let Some(mut packet) = batch.combustion_pressure {
        packet.set_channel(FastAdcChannel::PressureCombustion);
        enqueue(OutboundMessage::FastAdc(packet))?;
    }

    Ok(())
}

pub fn publish_slow_sensors(batch: SlowSensorsBatch) -> Result<(), PublishError> {
    if let Some(mut packet) = batch.battery_stand {
        packet.set_channel(SlowAdcChannel::BatteryStand);
        enqueue(OutboundMessage::SlowAdc(packet))?;
    }

    if let Some(mut packet) = batch.battery_computer {
        packet.set_channel(SlowAdcChannel::BatteryComputer);
        enqueue(OutboundMessage::SlowAdc(packet))?;
    }

    if let Some(mut packet) = batch.boost_voltage {
        packet.set_channel(SlowAdcChannel::BoostVoltage);
        enqueue(OutboundMessage::SlowAdc(packet))?;
    }

    if let Some(mut packet) = batch.starter_sense {
        packet.set_channel(SlowAdcChannel::StarterSense);
        enqueue(OutboundMessage::SlowAdc(packet))?;
    }

    if let Some(packet) = batch.servo {
        enqueue(OutboundMessage::ServoSensor(packet))?;
    }

    Ok(())
}

pub fn publish_temperature_sensor(packet: TempPacket) -> Result<(), PublishError> {
    enqueue(OutboundMessage::Temp(packet))
}

pub fn publish_armed_sensor(packet: ArmedPacket) -> Result<(), PublishError> {
    enqueue(OutboundMessage::Armed(packet))
}

pub fn publish_state_status(status: StateStatus) -> Result<(), PublishError> {
    enqueue(OutboundMessage::StateStatus(status))
}

pub fn publish_servo_status(status: ServoStatus) -> Result<(), PublishError> {
    enqueue(OutboundMessage::ServoStatus(status))
}

pub fn publish_command_status(status: CommandStatusPacket) -> Result<(), PublishError> {
    enqueue(OutboundMessage::CommandStatus(status))
}

pub(crate) async fn receive_outbound_message() -> OutboundMessage {
    OUTBOUND_QUEUE.receive().await
}

pub(crate) fn clear_outbound_queue() {
    OUTBOUND_QUEUE.clear();
}

fn enqueue(message: OutboundMessage) -> Result<(), PublishError> {
    match OUTBOUND_QUEUE.try_send(message) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(PublishError::QueueFull),
    }
}
