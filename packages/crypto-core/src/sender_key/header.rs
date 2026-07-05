//! The per-message header for a Sender Key chain: just this message's
//! position in the sender's chain — there is no DH ratchet key to name
//! (unlike [`crate::ratchet::MessageHeader`]) since a Sender Key chain
//! never re-keys itself from a DH exchange; it only ever gets replaced
//! outright by a fresh [`super::distribution::SenderKeyDistribution`],
//! e.g. after a membership change.

use crate::encoding::{varint_decode, varint_encode};
use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SenderKeyHeader {
    /// This message's index within the sender's chain.
    pub iteration: u32,
}

impl SenderKeyHeader {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5);
        varint_encode(self.iteration as u64, &mut out);
        out
    }

    pub fn decode(input: &[u8]) -> Result<Self> {
        Self::decode_with_len(input).map(|(header, _consumed)| header)
    }

    /// Like [`Self::decode`], but also returns how many bytes of `input`
    /// were the header — the rest is the signature-then-ciphertext that
    /// follows it in a combined wire buffer.
    pub fn decode_with_len(input: &[u8]) -> Result<(Self, usize)> {
        let (iteration, consumed) = varint_decode(input)?;
        Ok((Self { iteration: iteration as u32 }, consumed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_bytes() {
        let header = SenderKeyHeader { iteration: 300 };
        let encoded = header.encode();
        assert_eq!(SenderKeyHeader::decode(&encoded).unwrap(), header);
    }

    #[test]
    fn decode_with_len_reports_exactly_the_header_length_leaving_the_rest_untouched() {
        let header = SenderKeyHeader { iteration: 12 };
        let mut buffer = header.encode();
        let header_len = buffer.len();
        buffer.extend_from_slice(b"trailing signature+ciphertext bytes");

        let (decoded, consumed) = SenderKeyHeader::decode_with_len(&buffer).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(consumed, header_len);
        assert_eq!(&buffer[consumed..], b"trailing signature+ciphertext bytes");
    }

    #[test]
    fn rejects_empty_input() {
        assert!(SenderKeyHeader::decode(&[]).is_err());
    }
}
