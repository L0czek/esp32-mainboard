use bitfields::bitfield;

const REGISTER_POINTER_PREFIX: u8 = 0xA0;

/// TMP107 register addresses used by this driver (datasheet Table 3).
#[repr(u8)]
#[derive(Clone, Copy)]
pub(super) enum Register {
    /// Temperature register (`0x00`).
    Temperature = 0x00,
    /// Configuration register (`0x01`).
    Configuration = 0x01,
    /// ALERT1 high-limit register (`0x02`).
    HighLimit1 = 0x02,
    /// ALERT1 low-limit register (`0x03`).
    LowLimit1 = 0x03,
    /// ALERT2 high-limit register (`0x04`).
    HighLimit2 = 0x04,
    /// ALERT2 low-limit register (`0x05`).
    LowLimit2 = 0x05,
}

impl Register {
    /// Encodes TMP107 register pointer byte (`0101_P3P2P1P0`, datasheet Figure 21).
    pub(super) const fn pointer(self) -> u8 {
        REGISTER_POINTER_PREFIX | ((self as u8) & 0x0F)
    }
}

/// TMP107 configuration register (`0x01`) layout (datasheet Figure 32 / Table 5).
#[bitfield(u16, from = true)]
#[derive(Clone, Copy)]
pub struct ConfigRegisterBits {
    /// Bit D0 (reserved by datasheet).
    reserved0: bool,
    /// Bit D1 software reset control (`1` triggers reset).
    rst: bool,
    /// Bit D2 (reserved by datasheet).
    reserved2: bool,
    /// Bit D3 ALERT2 polarity.
    pol2: bool,
    /// Bit D4 ALERT2 therm/alert mode select.
    t2_a2: bool,
    /// Bit D5 ALERT2 low-limit flag (read-only in hardware).
    fl2: bool,
    /// Bit D6 ALERT2 high-limit flag (read-only in hardware).
    fh2: bool,
    /// Bit D7 ALERT1 polarity.
    pol1: bool,
    /// Bit D8 ALERT1 therm/alert mode select.
    t1_a1: bool,
    /// Bit D9 ALERT1 low-limit flag (read-only in hardware).
    fl1: bool,
    /// Bit D10 ALERT1 high-limit flag (read-only in hardware).
    fh1: bool,
    /// Bit D11 shutdown mode.
    sd: bool,
    /// Bit D12 one-shot conversion trigger.
    os: bool,
    /// Bits D15:D13 conversion-rate field.
    #[bits(3)]
    cr: u8,
}

/// Default shared config used by the firmware after init.
///
/// This keeps conversion-rate bits at `000` and enables shutdown + sets polarity
/// to have alert bits low
pub(super) fn default_config_register() -> ConfigRegisterBits {
    let mut config = ConfigRegisterBits::new();
    config.set_sd(true);
    config.set_t1_a1(true);
    config.set_pol1(true);
    config.set_t2_a2(true);
    config.set_pol2(true);
    config
}
