use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Instant;
use esp_hal::uart::{Config, UartTx};
use esp_hal::Blocking;
use mainboard::board::D3Pin;
use mainboard::tmp107::MAX_SENSORS;

use crate::config::BLACKBOX_BAUD_RATE;

const SYNC_BYTE: u8 = 0xAA;

const ID_FAST_ADC: u8 = 0x01;
const ID_SLOW_ADC: u8 = 0x02;
const ID_TEMPERATURE: u8 = 0x03;
const ID_DIGITAL: u8 = 0x04;
const ID_SERVO: u8 = 0x05;
const ID_LOG: u8 = 0x06;
const ID_HEARTBEAT: u8 = 0x07;

const CHANNEL_CAPACITY: usize = 32;

static BLACKBOX_CHANNEL: Channel<CriticalSectionRawMutex, BlackboxPacket, CHANNEL_CAPACITY> =
    Channel::new();

pub enum BlackboxPacket {
    Temperature {
        count: u8,
        timestamp_ms: u32,
        values: [u16; MAX_SENSORS],
    },
    Digital {
        timestamp_ms: u32,
        value: u8,
    },
    Servo {
        timestamp_ms: u32,
        ticks: u16,
    },
    Log {
        len: u8,
        data: [u8; 64],
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
    pub fn new(uart: esp_hal::peripherals::UART1<'static>, pin: D3Pin) -> Self {
        let tx = UartTx::new(uart, Config::default().with_baudrate(BLACKBOX_BAUD_RATE))
            .expect("UART1 blackbox init failed")
            .with_tx(pin);
        Self { tx }
    }

    pub fn write_fast_adc(&mut self, ts: u32, tensometer: u16, tank: u16, combustion: u16) {
        let mut buf = [0u8; 12];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_FAST_ADC;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        buf[6..8].copy_from_slice(&tensometer.to_le_bytes());
        buf[8..10].copy_from_slice(&tank.to_le_bytes());
        buf[10..12].copy_from_slice(&combustion.to_le_bytes());
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
        let mut buf = [0u8; 14];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_SLOW_ADC;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        buf[6..8].copy_from_slice(&bat_stand.to_le_bytes());
        buf[8..10].copy_from_slice(&bat_comp.to_le_bytes());
        buf[10..12].copy_from_slice(&boost.to_le_bytes());
        buf[12..14].copy_from_slice(&starter.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_heartbeat(&mut self) {
        let mut buf = [0u8; 6];
        buf[0] = SYNC_BYTE;
        buf[1] = ID_HEARTBEAT;
        let ts = Instant::now().as_millis() as u32;
        buf[2..6].copy_from_slice(&ts.to_le_bytes());
        self.write_all(&buf);
    }

    pub fn write_packet(&mut self, packet: &BlackboxPacket) {
        match packet {
            BlackboxPacket::Temperature {
                count,
                timestamp_ms,
                values,
            } => {
                let n = *count as usize;
                let total = 7 + n * 2;
                let mut buf = [0u8; 7 + MAX_SENSORS * 2];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_TEMPERATURE;
                buf[2] = *count;
                buf[3..7].copy_from_slice(&timestamp_ms.to_le_bytes());
                for i in 0..n {
                    let off = 7 + i * 2;
                    buf[off..off + 2].copy_from_slice(&values[i].to_le_bytes());
                }
                self.write_all(&buf[..total]);
            }
            BlackboxPacket::Digital {
                timestamp_ms,
                value,
            } => {
                let mut buf = [0u8; 7];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_DIGITAL;
                buf[2..6].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[6] = *value;
                self.write_all(&buf);
            }
            BlackboxPacket::Servo {
                timestamp_ms,
                ticks,
            } => {
                let mut buf = [0u8; 8];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_SERVO;
                buf[2..6].copy_from_slice(&timestamp_ms.to_le_bytes());
                buf[6..8].copy_from_slice(&ticks.to_le_bytes());
                self.write_all(&buf);
            }
            BlackboxPacket::Log { len, data } => {
                let n = *len as usize;
                let total = 3 + n;
                let mut buf = [0u8; 3 + 64];
                buf[0] = SYNC_BYTE;
                buf[1] = ID_LOG;
                buf[2] = *len;
                buf[3..3 + n].copy_from_slice(&data[..n]);
                self.write_all(&buf[..total]);
            }
        }
    }

    pub fn drain_channel(&mut self) {
        while let Ok(packet) = BLACKBOX_CHANNEL.try_receive() {
            self.write_packet(&packet);
        }
    }

    fn write_all(&mut self, buf: &[u8]) {
        let mut remaining = buf;
        while !remaining.is_empty() {
            match self.tx.write(remaining) {
                Ok(n) => remaining = &remaining[n..],
                Err(_) => break,
            }
        }
    }
}
