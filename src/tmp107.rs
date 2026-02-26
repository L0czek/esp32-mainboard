use defmt::info;
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::gpio::Output;
use esp_hal::uart::{UartRx, UartTx};
use esp_hal::Async;

// -- Protocol constants (VERIFY: datasheet docs/tmp107.pdf) --

/// Maximum sensors in a TMP107 daisy chain (5-bit address space).
pub const MAX_SENSORS: usize = 32;

/// Sent before every command so sensors can auto-detect baud rate.
const CALIBRATION_BYTE: u8 = 0x55;

/// Address Initialize command byte.
const ADDR_INIT_COMMAND: u8 = 0x95;

/// Temperature register address.
const TEMP_REGISTER: u8 = 0x00;

/// Timeout for a single sensor response (individual read).
const READ_TIMEOUT_MS: u64 = 10;

/// Timeout waiting for next address-init response before giving up.
const ADDR_DISCOVER_TIMEOUT_MS: u64 = 50;

/// Time for 3 TX bytes to physically leave the wire at 115200 baud
/// (3 * 10 bits / 115200 ≈ 260 µs). Rounded up for safety.
const TX_DRAIN_US: u64 = 500;

// -- Error type --

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum Tmp107Error {
    UartWrite,
    UartRead,
    Timeout,
    NoSensorsFound,
}

// -- Driver struct --

pub struct Tmp107 {
    tx: UartTx<'static, Async>,
    rx: UartRx<'static, Async>,
    dir: Output<'static>,
    sensor_count: u8,
}

impl Tmp107 {
    // -- Public API --

    /// Create driver, run Address Initialize, return configured driver
    /// with discovered sensor count.
    pub async fn init(
        tx: UartTx<'static, Async>,
        rx: UartRx<'static, Async>,
        dir: Output<'static>,
    ) -> Result<Self, Tmp107Error> {
        let mut driver = Self {
            tx,
            rx,
            dir,
            sensor_count: 0,
        };

        driver.discover_sensors().await?;
        Ok(driver)
    }

    pub fn sensor_count(&self) -> u8 {
        self.sensor_count
    }

    /// Read temperature from a single sensor by address (1-based).
    pub async fn read_temperature(
        &mut self,
        address: u8,
    ) -> Result<u16, Tmp107Error> {
        self.individual_read(address, TEMP_REGISTER).await
    }

    /// Read temperatures from all discovered sensors via global read.
    /// Returns the number of readings written to `out`.
    pub async fn read_all_temperatures(
        &mut self,
        out: &mut [u16],
    ) -> Result<usize, Tmp107Error> {
        self.global_read(self.sensor_count, TEMP_REGISTER, out)
            .await
    }

    // -- Address discovery --

    async fn discover_sensors(
        &mut self,
    ) -> Result<(), Tmp107Error> {
        let bytes = [CALIBRATION_BYTE, ADDR_INIT_COMMAND, 0x01];

        self.dir.set_high();
        self.tx
            .write_async(&bytes)
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        Timer::after_micros(TX_DRAIN_US).await;
        self.dir.set_low();

        let mut count: u8 = 0;
        let mut response = [0u8; 1];
        loop {
            match with_timeout(
                Duration::from_millis(ADDR_DISCOVER_TIMEOUT_MS),
                self.rx.read_async(&mut response),
            )
            .await
            {
                Ok(Ok(1)) => {
                    count += 1;
                    info!(
                        "TMP107 sensor {} discovered (byte: {:#04x})",
                        count, response[0]
                    );
                    if count as usize >= MAX_SENSORS {
                        break;
                    }
                }
                _ => break,
            }
        }

        if count == 0 {
            return Err(Tmp107Error::NoSensorsFound);
        }

        self.sensor_count = count;
        info!("TMP107 init complete: {} sensors", count);
        Ok(())
    }

    // -- Protocol helpers --

    fn command_byte(
        global: bool,
        read: bool,
        address: u8,
    ) -> u8 {
        let mut byte: u8 = address & 0x1F;
        if global {
            byte |= 0x80;
        }
        if read {
            byte |= 0x40;
        }
        byte
    }

    fn register_pointer(register: u8) -> u8 {
        ((register & 0x0F) << 4) | 0x05
    }

    async fn send_command(
        &mut self,
        global: bool,
        read: bool,
        address: u8,
        register: u8,
    ) -> Result<(), Tmp107Error> {
        let cmd = Self::command_byte(global, read, address);
        let ptr = Self::register_pointer(register);
        let bytes = [CALIBRATION_BYTE, cmd, ptr];

        self.dir.set_high();
        self.tx
            .write_async(&bytes)
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        Timer::after_micros(TX_DRAIN_US).await;
        self.dir.set_low();

        Ok(())
    }

    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), Tmp107Error> {
        let mut offset = 0;
        while offset < buf.len() {
            let n = with_timeout(
                Duration::from_millis(READ_TIMEOUT_MS),
                self.rx.read_async(&mut buf[offset..]),
            )
            .await
            .map_err(|_| Tmp107Error::Timeout)?
            .map_err(|_| Tmp107Error::UartRead)?;

            if n == 0 {
                return Err(Tmp107Error::UartRead);
            }
            offset += n;
        }
        Ok(())
    }

    // -- Protocol primitives --

    async fn individual_read(
        &mut self,
        address: u8,
        register: u8,
    ) -> Result<u16, Tmp107Error> {
        self.send_command(false, true, address, register).await?;
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf).await?;
        Ok(u16::from_le_bytes(buf))
    }

    async fn global_read(
        &mut self,
        max_address: u8,
        register: u8,
        out: &mut [u16],
    ) -> Result<usize, Tmp107Error> {
        let count = (max_address as usize).min(out.len());
        self.send_command(true, true, max_address, register)
            .await?;
        let mut buf = [0u8; 2];
        for slot in out.iter_mut().take(count) {
            self.read_exact(&mut buf).await?;
            *slot = u16::from_le_bytes(buf);
        }
        Ok(count)
    }

    async fn individual_write(
        &mut self,
        address: u8,
        register: u8,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        self.send_command(false, false, address, register).await?;
        self.dir.set_high();
        let bytes = value.to_le_bytes();
        self.tx
            .write_async(&bytes)
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        Timer::after_micros(TX_DRAIN_US).await;
        self.dir.set_low();
        Ok(())
    }

    async fn global_write(
        &mut self,
        max_address: u8,
        register: u8,
        value: u16,
    ) -> Result<(), Tmp107Error> {
        self.send_command(true, false, max_address, register)
            .await?;
        self.dir.set_high();
        let bytes = value.to_le_bytes();
        self.tx
            .write_async(&bytes)
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        Timer::after_micros(TX_DRAIN_US).await;
        self.dir.set_low();
        Ok(())
    }
}
