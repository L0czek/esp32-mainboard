use bitfields::bitfield;

/// TMP107 command/address phase bit layout (datasheet Figure 20 / Table 2).
#[bitfield(u8, from = true)]
#[derive(Clone, Copy)]
struct CommandAddressPhaseBits {
    /// Bit 0 (`G/nI`): global (`1`) or individual (`0`) operation.
    g_ni: bool,
    /// Bit 1 (`R/nW`): read (`1`) or write (`0`) operation.
    r_nw: bool,
    /// Bit 2 (`C/nA`): command (`1`) or address (`0`) encoding.
    c_na: bool,
    /// Bits 7:3 (`AC4..AC0` or `A4..A0`).
    #[bits(5)]
    ac: u8,
}

/// TMP107 command/address phase operations (datasheet Table 2).
#[derive(Clone, Copy)]
pub(super) enum Command {
    /// Address initialize command byte (`0x95`).
    AddressInitialize,
    /// Address initialize assignment byte (`G/nI=1, R/nW=0, C/nA=1, AC=start_address`).
    AddressInitializeAssign { start_address: u8 },
    /// Address phase for an individual read (`G/nI=0, R/nW=1, C/nA=0`).
    IndividualRead { address: u8 },
    /// Address phase for an individual write (`G/nI=0, R/nW=0, C/nA=0`).
    IndividualWrite { address: u8 },
    /// Address phase for a global read up to `max_address` (`G/nI=1, R/nW=1, C/nA=0`).
    GlobalRead { max_address: u8 },
    /// Address phase for a global write up to `max_address` (`G/nI=1, R/nW=0, C/nA=0`).
    GlobalWrite { max_address: u8 },
}

impl Command {
    /// Encodes one command/address phase byte.
    pub(super) fn byte(self) -> u8 {
        let mut phase = CommandAddressPhaseBits::new();

        match self {
            Self::AddressInitialize => {
                phase.set_g_ni(true);
                phase.set_r_nw(false);
                phase.set_c_na(true);
                phase.set_ac(0x12);
            }
            Self::AddressInitializeAssign { start_address } => {
                phase.set_g_ni(true);
                phase.set_r_nw(false);
                phase.set_c_na(true);
                phase.set_ac(start_address);
            }
            Self::IndividualRead { address } => {
                phase.set_g_ni(false);
                phase.set_r_nw(true);
                phase.set_c_na(false);
                phase.set_ac(address);
            }
            Self::IndividualWrite { address } => {
                phase.set_g_ni(false);
                phase.set_r_nw(false);
                phase.set_c_na(false);
                phase.set_ac(address);
            }
            Self::GlobalRead { max_address } => {
                phase.set_g_ni(true);
                phase.set_r_nw(true);
                phase.set_c_na(false);
                phase.set_ac(max_address);
            }
            Self::GlobalWrite { max_address } => {
                phase.set_g_ni(true);
                phase.set_r_nw(false);
                phase.set_c_na(false);
                phase.set_ac(max_address);
            }
        }

        phase.into()
    }
}
