use bitfields::bitfield;

const REGISTER_POINTER_PREFIX: u8 = 0xA0;

#[repr(u8)]
#[derive(Clone, Copy)]
pub(super) enum Register {
    Temperature = 0x00,
    Configuration = 0x01,
    HighLimit1 = 0x02,
    LowLimit1 = 0x03,
    HighLimit2 = 0x04,
    LowLimit2 = 0x05,
}

impl Register {
    pub(super) const fn pointer(self) -> u8 {
        REGISTER_POINTER_PREFIX | ((self as u8) & 0x0F)
    }
}

#[bitfield(u16, from = true)]
#[derive(Clone, Copy)]
pub(super) struct ConfigRegisterBits {
    #[bits(3)]
    _reserved0: u8,

    pol2: bool,
    t2_a2: bool,

    #[bits(2)]
    _reserved1: u8,

    pol1: bool,
    t1_a1: bool,

    #[bits(2)]
    _reserved2: u8,

    sd: bool,
    os: bool,

    #[bits(3)]
    _reserved3: u8,
}

pub(super) fn default_config_register() -> ConfigRegisterBits {
    let mut config = ConfigRegisterBits::new();
    config.set_sd(true);
    config.set_t1_a1(true);
    config.set_pol1(true);
    config.set_t2_a2(true);
    config.set_pol2(true);
    config
}
