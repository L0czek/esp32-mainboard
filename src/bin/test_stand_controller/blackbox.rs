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

const CHANNEL_CAPACITY: usize = 128;

static BLACKBOX_CHANNEL: Channel<CriticalSectionRawMutex, BlackboxPacket, CHANNEL_CAPACITY> =
    Channel::new();

pub enum BlackboxPacket {
    Temperature {
        sensor_id: u8,
        timestamp_ms: u32,
        value: u16,
    },
    Digital {
        timestamp_ms: u32,
        value: u8,
    },
    Servo {
        timestamp_ms: u32,
        ticks: u16,
    },
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

    pub fn write_fast_adc(&mut self, ts: u32, tensometer: u16, tank: u16, combustion: u16) {
        let mut buf = [0u8; 11];
        buf[0] = ID_FAST_ADC;
        buf[1..5].copy_from_slice(&ts.to_le_bytes());
        buf[5..7].copy_from_slice(&tensometer.to_le_bytes());
        buf[7..9].copy_from_slice(&tank.to_le_bytes());
        buf[9..11].copy_from_slice(&combustion.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_slow_adc(
        &mut self,
        ts: u32,
        bat_stand: u16,
        bat_comp: u16,
        boost: u16,
        starter: u16,
    ) {
        let mut buf = [0u8; 13];
        buf[0] = ID_SLOW_ADC;
        buf[1..5].copy_from_slice(&ts.to_le_bytes());
        buf[5..7].copy_from_slice(&bat_stand.to_le_bytes());
        buf[7..9].copy_from_slice(&bat_comp.to_le_bytes());
        buf[9..11].copy_from_slice(&boost.to_le_bytes());
        buf[11..13].copy_from_slice(&starter.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_packet(&mut self, packet: &BlackboxPacket) {
        match packet {
            BlackboxPacket::Temperature {
                sensor_id,
                timestamp_ms,
                value,
            } => {
                let mut buf = [0u8; 8];
                buf[0] = ID_TEMPERATURE;
                buf[1..5].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[5] = *sensor_id;
                buf[6..8].copy_from_slice(&value.to_le_bytes());
                self.write_all(&buf);
            }
            BlackboxPacket::Digital {
                timestamp_ms,
                value,
            } => {
                let mut buf = [0u8; 6];
                buf[0] = ID_DIGITAL;
                buf[1..5].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[5] = *value;
                self.write_all(&buf);
            }
            BlackboxPacket::Servo {
                timestamp_ms,
                ticks,
            } => {
                let mut buf = [0u8; 7];
                buf[0] = ID_SERVO;
                buf[1..5].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[5..7].copy_from_slice(&ticks.to_le_bytes());
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
                panic!("Blackbox UART TX FIFO full");
            }

            let written = self
                .tx
                .write(remaining)
                .expect("Blackbox UART write failed");

            if written == 0 {
                panic!("Blackbox UART write returned 0 with pending data");
            }

            remaining = &remaining[written..];
        }
    }
}
