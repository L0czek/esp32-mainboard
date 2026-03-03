use defmt::info;
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::uart::{RxError, TxError, UartRx, UartTx};
use esp_hal::Async;

/// Maximum sensors in a TMP107 daisy chain (5-bit address space).
pub const MAX_SENSORS: usize = 31;

/// Sent before every command so sensors can auto-detect baud rate.
const CALIBRATION_BYTE: u8 = 0x55;

/// Address Initialize command byte (G/nI=1, R/nW=0, C/nA=1, AC=10010).
const ADDR_INIT_COMMAND: u8 = 0x95;

/// Temperature register address.
const TEMP_REGISTER: u8 = 0x00;

/// Configuration register address.
const CONFIG_REGISTER: u8 = 0x01;

/// High limit 1 register address.
const HIGH_LIMIT_1_REGISTER: u8 = 0x02;

/// Low limit 1 register address.
const LOW_LIMIT_1_REGISTER: u8 = 0x03;

/// High limit 2 register address.
const HIGH_LIMIT_2_REGISTER: u8 = 0x04;

/// Low limit 2 register address.
const LOW_LIMIT_2_REGISTER: u8 = 0x05;

/// One-shot mode bit.
const CONFIG_OS_BIT: u16 = 1 << 12;

/// Shutdown mode bit.
const CONFIG_SD_BIT: u16 = 1 << 11;

/// Therm mode selection for ALERT1.
const CONFIG_T1_A1_BIT: u16 = 1 << 8;

/// ALERT1 polarity.
const CONFIG_POL1_BIT: u16 = 1 << 7;

/// Therm mode selection for ALERT2.
const CONFIG_T2_A2_BIT: u16 = 1 << 4;

/// ALERT2 polarity.
const CONFIG_POL2_BIT: u16 = 1 << 3;

/// Shared config used by all sensors: shutdown + therm mode + default LED off.
const DEFAULT_CONFIG_REGISTER: u16 =
    CONFIG_SD_BIT | CONFIG_T1_A1_BIT | CONFIG_POL1_BIT | CONFIG_T2_A2_BIT | CONFIG_POL2_BIT;

/// Maximum legal alert threshold (bits 1:0 must stay 0).
const TEMP_LIMIT_MAX: u16 = 0x7FFC;

/// Minimum legal alert threshold.
const TEMP_LIMIT_MIN: u16 = 0x8000;

/// One temperature LSB in the 16-bit register encoding.
const TEMP_RAW_LSB: u16 = 0x0004;

/// Any low limit other than TEMP_LIMIT_MIN keeps the alert block enabled.
const LED_ON_LOW_LIMIT: u16 = TEMP_LIMIT_MAX - TEMP_RAW_LSB;

/// Timeout for a single sensor response (individual read).
const READ_TIMEOUT_MS: u64 = 10;

/// Timeout waiting for next address-init response before giving up.
const ADDR_DISCOVER_TIMEOUT_MS: u64 = 50;

/// Datasheet recommended wait time between triggering one-shot temperature collection and reading temperature
pub const ONESHOT_CONVERSION_MS: u64 = 20;

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum Tmp107Error {
    UartWrite(TxError),
    UartRead(RxError),
    BufferTooSmall,
    Timeout,
    NoSensorsFound,
}

pub struct Tmp107 {
    tx: UartTx<'static, Async>,
    rx: UartRx<'static, Async>,
    sensor_count: u8,
    config_register: u16,
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
            config_register: DEFAULT_CONFIG_REGISTER,
        };

        loop {
            match driver.discover_sensors().await {
                Ok(()) => {
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
        self.individual_read(address, TEMP_REGISTER).await
    }

    /// Read temperatures from all discovered sensors via global read.
    /// Returns the number of readings written to `out`.
    /// Results are ordered by ascending address: out[0] = address 1.
    pub async fn read_all_temperatures(&mut self, out: &mut [u16]) -> Result<usize, Tmp107Error> {
        self.global_read(self.sensor_count, TEMP_REGISTER, out)
            .await?;
        Ok(self.sensor_count.into())
    }

    /// Put all sensors into shutdown mode (stops continuous conversion).
    /// Call once after init before starting one-shot collection loop.
    pub async fn shutdown(&mut self) -> Result<(), Tmp107Error> {
        self.write_global_config(CONFIG_SD_BIT, CONFIG_OS_BIT).await
    }

    /// Trigger a single temperature conversion on all sensors.
    /// Sensors return to shutdown mode after conversion completes.
    /// Wait at least 20ms before reading results.
    pub async fn trigger_one_shot(&mut self) -> Result<(), Tmp107Error> {
        self.write_global_config(CONFIG_SD_BIT | CONFIG_OS_BIT, 0)
            .await
    }

    /// Set ALERT1/ALERT2 LEDs on a single sensor using per-sensor limit registers.
    /// All sensors keep the same shared config register value.
    pub async fn set_leds(
        &mut self,
        address: u8,
        led1: bool,
        led2: bool,
    ) -> Result<(), Tmp107Error> {
        self.set_led_output(address, HIGH_LIMIT_1_REGISTER, LOW_LIMIT_1_REGISTER, led1)
            .await?;
        self.set_led_output(address, HIGH_LIMIT_2_REGISTER, LOW_LIMIT_2_REGISTER, led2)
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

    async fn discover_sensors(&mut self) -> Result<(), Tmp107Error> {
        let addr_assign = Self::addr_init_byte(0x01);
        let bytes = [CALIBRATION_BYTE, ADDR_INIT_COMMAND, addr_assign];

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
        Ok(())
    }

    async fn write_global_config(
        &mut self,
        set_bits: u16,
        clear_bits: u16,
    ) -> Result<(), Tmp107Error> {
        let config = (self.config_register | set_bits) & !clear_bits;
        self.config_register = config & !CONFIG_OS_BIT;
        self.global_write(self.sensor_count, CONFIG_REGISTER, config)
            .await
    }

    async fn set_led_output(
        &mut self,
        address: u8,
        high_register: u8,
        low_register: u8,
        led_on: bool,
    ) -> Result<(), Tmp107Error> {
        self.individual_write(address, high_register, TEMP_LIMIT_MAX)
            .await?;
        let low_limit = if led_on {
            LED_ON_LOW_LIMIT
        } else {
            TEMP_LIMIT_MIN
        };
        self.individual_write(address, low_register, low_limit)
            .await
    }

    // -- Protocol helpers --

    /// Build command/address byte per datasheet Table 2:
    /// bit 0 = G/nI, bit 1 = R/nW, bit 2 = C/nA (0 for normal ops),
    /// bits 3-7 = AC0-AC4 (5-bit device address).
    fn command_byte(global: bool, read: bool, address: u8) -> u8 {
        let mut byte = (address & 0x1F) << 3;
        if global {
            byte |= 0x01;
        }
        if read {
            byte |= 0x02;
        }
        byte
    }

    /// Build address-init assign byte:
    /// G/nI=1, R/nW=0, C/nA=1, starting address in bits 3-7.
    fn addr_init_byte(address: u8) -> u8 {
        0x05 | ((address & 0x1F) << 3)
    }

    /// Build register pointer byte per datasheet Figure 21:
    /// bits 3-0 = P3-P0 (register address), bits 7-4 = 0101.
    fn register_pointer(register: u8) -> u8 {
        (register & 0x0F) | 0xA0
    }

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
        let mut clearnig_buffer = [0u8; 64];
        self.rx
            .read_buffered(&mut clearnig_buffer)
            .map_err(Tmp107Error::UartRead)?;
        Ok(())
    }

    async fn read_exact(&mut self, buf: &mut [u8], len: usize) -> Result<(), Tmp107Error> {
        with_timeout(
            Duration::from_millis(READ_TIMEOUT_MS),
            self.rx.read_exact_async(&mut buf[..len]),
        )
        .await
        .map_err(|_| Tmp107Error::Timeout)?
        .map_err(Tmp107Error::UartRead)?;
        Ok(())
    }

    // -- Protocol primitives --

    async fn individual_read(&mut self, address: u8, register: u8) -> Result<u16, Tmp107Error> {
        let cmd = Self::command_byte(false, true, address);
        let ptr = Self::register_pointer(register);

        self.clear_read_buffer()?;

        self.tx(&[CALIBRATION_BYTE, cmd, ptr]).await?;

        let mut buf = [0u8; 2];
        self.read_exact(&mut buf, 2).await?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Reads all sensors up to max_address. max_address is the number of sensors
    /// queried (we number sensors from 1)
    /// out must fit all the data (be at least max_address len)
    async fn global_read(
        &mut self,
        max_address: u8,
        register: u8,
        out: &mut [u16],
    ) -> Result<(), Tmp107Error> {
        if out.len() < max_address.into() {
            return Err(Tmp107Error::BufferTooSmall);
        }

        let count = max_address as usize;
        let cmd = Self::command_byte(true, true, max_address);
        let ptr = Self::register_pointer(register);

        self.clear_read_buffer()?;

        self.tx(&[CALIBRATION_BYTE, cmd, ptr]).await?;

        let mut buf = [0u8; MAX_SENSORS * 2];

        self.read_exact(&mut buf, count * 2).await?;

        for i in 0..count {
            // Responses arrive highest-address-first (datasheet Figure 29);
            out[count - 1 - i] = u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]])
        }

        Ok(())
    }

    /// Write to a single sensor. Data is sent in the same TX burst
    /// as the command and register pointer (datasheet Figure 26).
    async fn individual_write(
        &mut self,
        address: u8,
        register: u8,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        let cmd = Self::command_byte(false, false, address);
        let ptr = Self::register_pointer(register);
        let data = value.to_le_bytes();
        let bytes = [CALIBRATION_BYTE, cmd, ptr, data[0], data[1]];

        self.tx(&bytes).await
    }

    /// Write to all sensors up to max_address. Data is sent in the
    /// same TX burst (datasheet Figure 28).
    async fn global_write(
        &mut self,
        max_address: u8,
        register: u8,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        let cmd = Self::command_byte(true, false, max_address);
        let ptr = Self::register_pointer(register);
        let data = value.to_le_bytes();
        let bytes = [CALIBRATION_BYTE, cmd, ptr, data[0], data[1]];

        self.tx(&bytes).await
    }
}
