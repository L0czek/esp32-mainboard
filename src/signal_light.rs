use embedded_hal::i2c::I2c;
use pcf857x::Pcf8574;

/// Configuration for a PCF8574-driven signal light tower.
///
/// Each bool field represents an output: `true` = on, `false` = off.
/// The PCF8574 outputs are active-low, so the register conversion
/// inverts the logic (0 = on, 1 = off).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct SignalLightConfig {
    pub green: bool,
    pub red: bool,
    pub yellow: bool,
    pub blue: bool,
    pub white: bool,
    pub buzzer: bool,
}

impl SignalLightConfig {
    /// Convert to PCF8574 register byte.
    /// All outputs are active-low: 0 = on, 1 = off.
    /// Bits 0-1 (reserved) are kept high (off).
    /// Bit layout (MSB first): green red yellow blue white buzzer _ _
    pub fn to_register(self) -> u8 {
        let mut reg: u8 = 0xFF;
        if self.green {
            reg &= !(1 << 7);
        }
        if self.red {
            reg &= !(1 << 6);
        }
        if self.yellow {
            reg &= !(1 << 5);
        }
        if self.blue {
            reg &= !(1 << 4);
        }
        if self.white {
            reg &= !(1 << 3);
        }
        if self.buzzer {
            reg &= !(1 << 2);
        }
        reg
    }

    /// Parse from PCF8574 register byte (active-low).
    pub fn from_register(reg: u8) -> Self {
        Self {
            green: reg & (1 << 7) == 0,
            red: reg & (1 << 6) == 0,
            yellow: reg & (1 << 5) == 0,
            blue: reg & (1 << 4) == 0,
            white: reg & (1 << 3) == 0,
            buzzer: reg & (1 << 2) == 0,
        }
    }
}

pub struct SignalLight<I2C: I2c> {
    expander: Pcf8574<I2C>,
    current: SignalLightConfig,
}

impl<I2C: I2c> SignalLight<I2C> {
    pub fn new(i2c: I2C, address: pcf857x::SlaveAddr) -> Result<Self, pcf857x::Error<I2C::Error>> {
        let mut expander = Pcf8574::new(i2c, address);
        let config = SignalLightConfig::default();
        expander.set(config.to_register())?;
        Ok(Self {
            expander,
            current: config,
        })
    }

    pub fn set(&mut self, config: SignalLightConfig) -> Result<(), pcf857x::Error<I2C::Error>> {
        self.expander.set(config.to_register())?;
        self.current = config;
        Ok(())
    }

    pub fn current(&self) -> SignalLightConfig {
        self.current
    }
}
