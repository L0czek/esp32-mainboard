pub mod digital;
pub mod fast;
pub mod slow;
pub mod status;
pub mod temp;

use crate::mqtt::codec::EncodeError;

pub trait EncodablePayload {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError>;
}

pub trait EncodableEnum
where
    Self: core::marker::Sized,
{
    fn as_str(&self) -> &'static str;

    fn as_bytes(&self) -> &'static [u8] {
        self.as_str().as_bytes()
    }
}
