//! The per-message header: tells the recipient which DH ratchet key the
//! sender was using, how many messages were in the sender's *previous*
//! sending chain, and this message's position in the *current* one. This
//! is exactly what a recipient needs to detect a DH ratchet step and to
//! recover skipped message keys for out-of-order delivery.

use x25519_dalek::PublicKey as X25519PublicKey;

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageHeader {
    /// The sender's current DH ratchet public key.
    pub dh_public: [u8; 32],
    /// Number of messages sent in the sender's previous sending chain
    /// (lets the recipient know how many skipped keys to expect there).
    pub previous_chain_length: u32,
    /// This message's index within the sender's current sending chain.
    pub message_number: u32,
}

impl MessageHeader {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 10);
        out.extend_from_slice(&self.dh_public);
        varint_encode(self.previous_chain_length as u64, &mut out);
        varint_encode(self.message_number as u64, &mut out);
        out
    }

    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.len() < 32 {
            return Err(CryptoError::Decode("header shorter than a public key"));
        }
        let dh_public: [u8; 32] = input[..32].try_into().unwrap();
        let (previous_chain_length, consumed) = varint_decode(&input[32..])?;
        let (message_number, _consumed2) = varint_decode(&input[32 + consumed..])?;
        Ok(Self {
            dh_public,
            previous_chain_length: previous_chain_length as u32,
            message_number: message_number as u32,
        })
    }

    pub fn dh_public_key(&self) -> X25519PublicKey {
        X25519PublicKey::from(self.dh_public)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_bytes() {
        let header = MessageHeader {
            dh_public: [9u8; 32],
            previous_chain_length: 12,
            message_number: 300,
        };
        let encoded = header.encode();
        let decoded = MessageHeader::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn rejects_truncated_header() {
        let header = MessageHeader {
            dh_public: [1u8; 32],
            previous_chain_length: 0,
            message_number: 0,
        };
        let mut encoded = header.encode();
        encoded.truncate(10);
        assert!(MessageHeader::decode(&encoded).is_err());
    }
}
