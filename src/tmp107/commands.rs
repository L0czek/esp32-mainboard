const ADDRESS_MASK: u8 = 0x1F;
const GLOBAL_BIT: u8 = 1 << 0;
const READ_BIT: u8 = 1 << 1;
const COMMAND_ADDRESS_BIT: u8 = 1 << 2;

#[derive(Clone, Copy)]
pub(super) enum Command {
    AddressInitialize,
    AddressInitializeAssign { start_address: u8 },
    IndividualRead { address: u8 },
    IndividualWrite { address: u8 },
    GlobalRead { max_address: u8 },
    GlobalWrite { max_address: u8 },
}

impl Command {
    pub(super) const fn byte(self) -> u8 {
        match self {
            Self::AddressInitialize => Self::encode(true, false, true, 0x12),
            Self::AddressInitializeAssign { start_address } => {
                Self::encode(true, false, true, start_address)
            }
            Self::IndividualRead { address } => Self::encode(false, true, false, address),
            Self::IndividualWrite { address } => Self::encode(false, false, false, address),
            Self::GlobalRead { max_address } => Self::encode(true, true, false, max_address),
            Self::GlobalWrite { max_address } => Self::encode(true, false, false, max_address),
        }
    }

    const fn encode(global: bool, read: bool, command_address: bool, address: u8) -> u8 {
        let mut byte = (address & ADDRESS_MASK) << 3;

        if global {
            byte |= GLOBAL_BIT;
        }
        if read {
            byte |= READ_BIT;
        }
        if command_address {
            byte |= COMMAND_ADDRESS_BIT;
        }

        byte
    }
}
