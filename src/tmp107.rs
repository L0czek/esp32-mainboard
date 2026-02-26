use defmt::info;
use embassy_time::{with_timeout, Duration};
use esp_hal::gpio::Output;
use esp_hal::uart::{UartRx, UartTx};
use esp_hal::Async;

/// Maximum sensors in a TMP107 daisy chain (5-bit address space).
pub const MAX_SENSORS: usize = 32;

/// Sent before every command so sensors can auto-detect baud rate.
const CALIBRATION_BYTE: u8 = 0x55;

/// Address Initialize command byte (G/nI=1, R/nW=0, C/nA=1, AC=10010).
const ADDR_INIT_COMMAND: u8 = 0x95;

/// Temperature register address.
const TEMP_REGISTER: u8 = 0x00;

/// Timeout for a single sensor response (individual read).
const READ_TIMEOUT_MS: u64 = 10;

/// Timeout waiting for next address-init response before giving up.
const ADDR_DISCOVER_TIMEOUT_MS: u64 = 50;

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum Tmp107Error {
    UartWrite,
    UartRead,
    Timeout,
    NoSensorsFound,
}

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
    /// Results are ordered by ascending address: out[0] = address 1.
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
        let addr_assign = Self::addr_init_byte(0x01);
        let bytes = [CALIBRATION_BYTE, ADDR_INIT_COMMAND, addr_assign];

        self.dir.set_high();
        self.tx_flush(&bytes).await?;
        self.dir.set_low();

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
                Ok(Err(_)) => return Err(Tmp107Error::UartRead),
            }
            count += 1;
            info!(
                "TMP107 sensor {} discovered (byte: {:#04x})",
                count, response[0]
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

    // -- Protocol helpers --

    /// Build command/address byte per datasheet Table 2:
    /// bit 0 = G/nI, bit 1 = R/nW, bit 2 = C/nA (0 for normal ops),
    /// bits 3-7 = AC0-AC4 (5-bit device address).
    fn command_byte(
        global: bool,
        read: bool,
        address: u8,
    ) -> u8 {
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
    /// bits 7-4 = P3-P0 (register address), bits 3-0 = 0101.
    fn register_pointer(register: u8) -> u8 {
        ((register & 0x0F) << 4) | 0x05
    }

    /// Transmit bytes and wait for all bits to leave the wire.
    async fn tx_flush(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Tmp107Error> {
        self.tx
            .write_async(bytes)
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        self.tx
            .flush_async()
            .await
            .map_err(|_| Tmp107Error::UartWrite)?;
        Ok(())
    }

    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), Tmp107Error> {
        with_timeout(
            Duration::from_millis(READ_TIMEOUT_MS),
            self.rx.read_exact_async(buf),
        )
        .await
        .map_err(|_| Tmp107Error::Timeout)?
        .map_err(|_| Tmp107Error::UartRead)?;
        Ok(())
    }

    // -- Protocol primitives --

    async fn individual_read(
        &mut self,
        address: u8,
        register: u8,
    ) -> Result<u16, Tmp107Error> {
        let cmd = Self::command_byte(false, true, address);
        let ptr = Self::register_pointer(register);

        self.dir.set_high();
        self.tx_flush(&[CALIBRATION_BYTE, cmd, ptr]).await?;
        self.dir.set_low();

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
        let cmd = Self::command_byte(true, true, max_address);
        let ptr = Self::register_pointer(register);

        self.dir.set_high();
        self.tx_flush(&[CALIBRATION_BYTE, cmd, ptr]).await?;
        self.dir.set_low();

        let mut buf = [0u8; 2];
        for slot in out.iter_mut().take(count) {
            self.read_exact(&mut buf).await?;
            *slot = u16::from_le_bytes(buf);
        }

        // Responses arrive highest-address-first (datasheet Figure 29);
        // reverse so out[0] = address 1 (ascending order).
        out[..count].reverse();

        Ok(count)
    }

    /// Write to a single sensor. Data is sent in the same TX burst
    /// as the command and register pointer (datasheet Figure 26).
    #[allow(dead_code)]
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

        self.dir.set_high();
        self.tx_flush(&bytes).await?;
        self.dir.set_low();

        Ok(())
    }

    /// Write to all sensors up to max_address. Data is sent in the
    /// same TX burst (datasheet Figure 28).
    #[allow(dead_code)]
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

        self.dir.set_high();
        self.tx_flush(&bytes).await?;
        self.dir.set_low();

        Ok(())
    }
}
