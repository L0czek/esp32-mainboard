mod commands;
mod registers;

use crate::tmp107::commands::Command;
use crate::tmp107::registers::{default_config_register, ConfigRegisterBits, Register};
use defmt::info;
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::uart::{RxError, TxError, UartRx, UartTx};
use esp_hal::Async;

/// Maximum sensors in a TMP107 daisy chain (5-bit address space).
pub const MAX_SENSORS: usize = 31;

/// Sent before every command so sensors can auto-detect baud rate.
const CALIBRATION_BYTE: u8 = 0x55;

/// Maximum legal alert threshold (bits 1:0 must stay 0).
const TEMP_LIMIT_MAX: u16 = 0x7FFC;

/// Minimum legal alert threshold.
const TEMP_LIMIT_MIN: u16 = 0x8000;

/// Timeout for a single sensor response (individual read).
const READ_TIMEOUT_MS: u64 = 10;

/// Timeout waiting for next address-init response before giving up.
const ADDR_DISCOVER_TIMEOUT_MS: u64 = 50;

/// Datasheet recommended wait time between triggering one-shot temperature collection and reading
/// temperature.
pub const ONESHOT_CONVERSION_MS: u64 = 20;

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum Tmp107Error {
    UartWrite(TxError),
    UartRead(RxError),
    BufferTooSmall,
    Timeout,
    NoSensorsFound,
    Other(&'static str),
}

pub struct Tmp107 {
    tx: UartTx<'static, Async>,
    rx: UartRx<'static, Async>,
    sensor_count: u8,
    config_register: ConfigRegisterBits,
}

impl Tmp107 {
    // -- Public API --

    /// Create driver, run Address Initialize, return configured driver
    /// with discovered sensor count.
    pub async fn init(
        tx: UartTx<'static, Async>,
        rx: UartRx<'static, Async>,
    ) -> Result<Self, Tmp107Error> {
        let mut driver = Self {
            tx,
            rx,
            sensor_count: 0,
            config_register: default_config_register(),
        };

        loop {
            match driver.address_initialize().await {
                Ok(_) => {
                    break;
                }
                Err(_) => {
                    info!("No sensors discovered");
                    Timer::after_millis(100).await;
                }
            }
        }
        Ok(driver)
    }

    pub fn sensor_count(&self) -> u8 {
        self.sensor_count
    }

    /// Read temperature from a single sensor by address (1-based).
    pub async fn read_temperature(&mut self, address: u8) -> Result<u16, Tmp107Error> {
        self.individual_read(address, Register::Temperature).await
    }

    /// Read temperatures from all discovered sensors via global read.
    /// Returns the number of readings written to `out`.
    /// Results are ordered by ascending address: out[0] = address 1.
    pub async fn read_all_temperatures(&mut self, out: &mut [u16]) -> Result<usize, Tmp107Error> {
        self.global_read(self.sensor_count, Register::Temperature, out)
            .await?;
        Ok(self.sensor_count.into())
    }

    /// Put all sensors into shutdown mode (stops continuous conversion).
    /// Call once after init before starting one-shot collection loop.
    pub async fn shutdown(&mut self) -> Result<(), Tmp107Error> {
        self.write_global_config(|config| {
            config.set_sd(true);
            config.set_os(false);
        })
        .await
    }

    /// Trigger a single temperature conversion on all sensors.
    /// Sensors return to shutdown mode after conversion completes.
    /// Wait at least 20ms before reading results.
    pub async fn trigger_one_shot(&mut self) -> Result<(), Tmp107Error> {
        self.write_global_config(|config| {
            config.set_sd(true);
            config.set_os(true);
        })
        .await
    }

    pub async fn set_alert_polarity(
        &mut self,
        alert: u8,
        polarity: bool,
    ) -> Result<(), Tmp107Error> {
        match alert {
            1 => {
                self.write_global_config(|config| {
                    config.set_pol1(polarity);
                })
                .await
            }
            2 => {
                self.write_global_config(|config| {
                    config.set_pol2(polarity);
                })
                .await
            }
            _ => Err(Tmp107Error::Other("Invalid alert number")),
        }
    }

    /// Set ALERT1/ALERT2 LEDs on a single sensor using per-sensor limit registers.
    /// All sensors keep the same shared config register value.
    pub async fn set_leds(
        &mut self,
        address: u8,
        led1: bool,
        led2: bool,
    ) -> Result<(), Tmp107Error> {
        self.set_led_output(address, Register::HighLimit1, Register::LowLimit1, led1)
            .await?;
        self.set_led_output(address, Register::HighLimit2, Register::LowLimit2, led2)
            .await
    }

    /// Show the two lowest bits of each sensor's address on its
    /// ALERT LEDs. ALERT1 = bit 0, ALERT2 = bit 1.
    pub async fn show_address_leds(&mut self) -> Result<(), Tmp107Error> {
        for addr in 1..=self.sensor_count {
            let led1 = (addr & 0x01) != 0;
            let led2 = (addr & 0x02) != 0;
            self.set_leds(addr, led1, led2).await?;
        }
        Ok(())
    }

    // -- Address discovery --
    pub async fn address_initialize(&mut self) -> Result<u8, Tmp107Error> {
        let bytes = [
            CALIBRATION_BYTE,
            Command::AddressInitialize.byte(),
            Command::AddressInitializeAssign {
                start_address: 0x01,
            }
            .byte(),
        ];

        self.clear_read_buffer()?;
        self.tx(&bytes).await?;

        let mut count: u8 = 0;
        let mut response = [0u8; 1];
        loop {
            match with_timeout(
                Duration::from_millis(ADDR_DISCOVER_TIMEOUT_MS),
                self.rx.read_exact_async(&mut response),
            )
            .await
            {
                Ok(Ok(())) => {}
                Err(_) => break, // Timeout: no more sensors
                Ok(Err(e)) => return Err(Tmp107Error::UartRead(e)),
            }
            count += 1;
            info!(
                "TMP107 sensor {} discovered (byte: {:#04x}, address: {})",
                count,
                response[0],
                response[0] >> 3
            );
            if count as usize >= MAX_SENSORS {
                break;
            }
        }

        if count == 0 {
            return Err(Tmp107Error::NoSensorsFound);
        }

        self.sensor_count = count;
        info!("TMP107 init complete: {} sensors", count);
        Ok(count)
    }

    async fn write_global_config(
        &mut self,
        mutate: impl FnOnce(&mut ConfigRegisterBits),
    ) -> Result<(), Tmp107Error> {
        let mut next = self.config_register;
        mutate(&mut next);

        let config_to_write: u16 = next.into();
        next.set_os(false); // do not persist One-shot bit in remembered config
        self.config_register = next;

        self.global_write(self.sensor_count, Register::Configuration, config_to_write)
            .await
    }

    async fn set_led_output(
        &mut self,
        address: u8,
        high_register: Register,
        low_register: Register,
        led_on: bool,
    ) -> Result<(), Tmp107Error> {
        self.individual_write(
            address,
            high_register,
            if led_on {
                TEMP_LIMIT_MIN
            } else {
                TEMP_LIMIT_MAX
            },
        )
        .await?;

        self.individual_write(
            address,
            low_register,
            if led_on {
                TEMP_LIMIT_MIN
            } else {
                TEMP_LIMIT_MAX
            },
        )
        .await
    }

    // -- Protocol helpers --

    /// Transmit bytes and wait for all bits to leave the wire.
    async fn tx(&mut self, bytes: &[u8]) -> Result<(), Tmp107Error> {
        let mut to_write = bytes;

        while !to_write.is_empty() {
            let bytes_written = self
                .tx
                .write_async(to_write)
                .await
                .map_err(Tmp107Error::UartWrite)?;
            to_write = &to_write[bytes_written..];
        }

        Ok(())
    }

    fn clear_read_buffer(&mut self) -> Result<(), Tmp107Error> {
        let mut clearing_buffer = [0u8; 64];
        self.rx
            .read_buffered(&mut clearing_buffer)
            .map_err(Tmp107Error::UartRead)?;
        Ok(())
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Tmp107Error> {
        with_timeout(
            Duration::from_millis(READ_TIMEOUT_MS),
            self.rx.read_exact_async(buf),
        )
        .await
        .map_err(|_| Tmp107Error::Timeout)?
        .map_err(Tmp107Error::UartRead)?;
        Ok(())
    }

    // -- Protocol primitives --

    async fn individual_read(
        &mut self,
        address: u8,
        register: Register,
    ) -> Result<u16, Tmp107Error> {
        let cmd = Command::IndividualRead { address }.byte();
        let ptr = register.pointer();

        self.clear_read_buffer()?;

        self.tx(&[CALIBRATION_BYTE, cmd, ptr]).await?;

        let mut buf = [0u8; 2];
        self.read_exact(&mut buf).await?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Reads all sensors up to max_address. max_address is the number of sensors
    /// queried (we number sensors from 1)
    /// out must fit all the data (be at least max_address len)
    async fn global_read(
        &mut self,
        max_address: u8,
        register: Register,
        out: &mut [u16],
    ) -> Result<(), Tmp107Error> {
        if out.len() < max_address.into() {
            return Err(Tmp107Error::BufferTooSmall);
        }

        let count = max_address as usize;
        let cmd = Command::GlobalRead { max_address }.byte();
        let ptr = register.pointer();

        self.clear_read_buffer()?;

        self.tx(&[CALIBRATION_BYTE, cmd, ptr]).await?;

        let mut buf = [0u8; MAX_SENSORS * 2];
        self.read_exact(&mut buf[..count * 2]).await?;

        for i in 0..count {
            // Responses arrive highest-address-first (datasheet Figure 29).
            out[count - 1 - i] = u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]])
        }

        Ok(())
    }

    /// Write to a single sensor. Data is sent in the same TX burst
    /// as the command and register pointer (datasheet Figure 26).
    async fn individual_write(
        &mut self,
        address: u8,
        register: Register,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        let cmd = Command::IndividualWrite { address }.byte();
        let ptr = register.pointer();
        let data = value.to_le_bytes();
        let bytes = [CALIBRATION_BYTE, cmd, ptr, data[0], data[1]];

        self.tx(&bytes).await
    }

    /// Write to all sensors up to max_address. Data is sent in the
    /// same TX burst (datasheet Figure 28).
    async fn global_write(
        &mut self,
        max_address: u8,
        register: Register,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        let cmd = Command::GlobalWrite { max_address }.byte();
        let ptr = register.pointer();
        let data = value.to_le_bytes();
        let bytes = [CALIBRATION_BYTE, cmd, ptr, data[0], data[1]];

        self.tx(&bytes).await
    }
}
