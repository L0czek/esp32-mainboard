pub mod digital;
pub mod fast;
pub mod slow;
pub mod status;
pub mod temp;

use crate::mqtt::codec::EncodeError;

pub trait EncodablePayload {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError>;
}
