use crate::config::BLACKBOX_BAUD_RATE;
use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_hal::uart::{Config, UartTx};
use esp_hal::Blocking;
use mainboard::board::D4Pin;

const ID_FAST_ADC: u8 = 0x01;
const ID_SLOW_ADC: u8 = 0x02;
const ID_TEMPERATURE: u8 = 0x03;
const ID_DIGITAL: u8 = 0x04;
const ID_SERVO: u8 = 0x05;
const ID_TIMING_SYNC: u8 = 0x06;
const TIMING_SYNC_MARKER: [u8; 7] = *b"TIMESYN";

const CHANNEL_CAPACITY: usize = 128;

static BLACKBOX_CHANNEL: Channel<CriticalSectionRawMutex, BlackboxPacket, CHANNEL_CAPACITY> =
    Channel::new();

pub enum BlackboxPacket {
    Temperature { sensor_id: u8, value: u16 },
    Digital { value: u8 },
    Servo { ticks: u16 },
}

pub fn send_to_blackbox(packet: BlackboxPacket) {
    if BLACKBOX_CHANNEL.try_send(packet).is_err() {
        warn!("Dropping blackbox packet: channel full");
    }
}

pub struct BlackboxWriter {
    tx: UartTx<'static, Blocking>,
}

impl BlackboxWriter {
    pub fn new(uart: esp_hal::peripherals::UART1<'static>, pin: D4Pin) -> Self {
        let tx = UartTx::new(uart, Config::default().with_baudrate(BLACKBOX_BAUD_RATE))
            .expect("UART1 blackbox init failed")
            .with_tx(pin);
        Self { tx }
    }

    pub fn write_timing_sync(&mut self, timestamp_ms: u32, fast_interval_ms: u16) {
        let mut buf = [0u8; 14];
        buf[0] = ID_TIMING_SYNC;
        buf[1..8].copy_from_slice(&TIMING_SYNC_MARKER);
        buf[8..12].copy_from_slice(&timestamp_ms.to_le_bytes());
        buf[12..14].copy_from_slice(&fast_interval_ms.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_fast_adc(&mut self, tensometer: u16, tank: u16, combustion: u16) {
        let mut buf = [0u8; 7];
        buf[0] = ID_FAST_ADC;
        buf[1..3].copy_from_slice(&tensometer.to_le_bytes());
        buf[3..5].copy_from_slice(&tank.to_le_bytes());
        buf[5..7].copy_from_slice(&combustion.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_slow_adc(&mut self, bat_stand: u16, bat_comp: u16, boost: u16, starter: u16) {
        let mut buf = [0u8; 9];
        buf[0] = ID_SLOW_ADC;
        buf[1..3].copy_from_slice(&bat_stand.to_le_bytes());
        buf[3..5].copy_from_slice(&bat_comp.to_le_bytes());
        buf[5..7].copy_from_slice(&boost.to_le_bytes());
        buf[7..9].copy_from_slice(&starter.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_packet(&mut self, packet: &BlackboxPacket) {
        match packet {
            BlackboxPacket::Temperature { sensor_id, value } => {
                let mut buf = [0u8; 4];
                buf[0] = ID_TEMPERATURE;
                buf[1] = *sensor_id;
                buf[2..4].copy_from_slice(&value.to_le_bytes());
                self.write_all(&buf);
            }
            BlackboxPacket::Digital { value } => {
                let mut buf = [0u8; 2];
                buf[0] = ID_DIGITAL;
                buf[1] = *value;
                self.write_all(&buf);
            }
            BlackboxPacket::Servo { ticks } => {
                let mut buf = [0u8; 3];
                buf[0] = ID_SERVO;
                buf[1..3].copy_from_slice(&ticks.to_le_bytes());
                self.write_all(&buf);
            }
        }
    }

    pub fn send_one_from_channel(&mut self) {
        if let Ok(packet) = BLACKBOX_CHANNEL.try_receive() {
            self.write_packet(&packet);
        }
    }

    fn write_all(&mut self, buf: &[u8]) {
        let mut remaining = buf;
        while !remaining.is_empty() {
            if !self.tx.write_ready() {
                warn!("Blackbox UART FIFO full, busy looping to write packet");
            }

            let written = self
                .tx
                .write(remaining)
                .expect("Blackbox UART write failed");

            remaining = &remaining[written..];
        }
    }
}
