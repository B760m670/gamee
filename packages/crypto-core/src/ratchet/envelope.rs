//! Wire framing for a single ratchet message: the [`MessageHeader`] and its
//! ciphertext travel together as one blob over the network, so callers only
//! need to hand one `Vec<u8>` to their transport instead of juggling two
//! values in lockstep.

use crate::error::{CryptoError, Result};

use super::header::MessageHeader;

/// Concatenates an encoded header with its ciphertext. The header is
/// self-delimiting (see [`MessageHeader::decode_with_len`]), so no extra
/// length prefix is needed.
pub fn encode(header: &MessageHeader, ciphertext: &[u8]) -> Vec<u8> {
    let mut out = header.encode();
    out.extend_from_slice(ciphertext);
    out
}

/// Splits a wire message back into its header and ciphertext.
pub fn decode(input: &[u8]) -> Result<(MessageHeader, &[u8])> {
    let (header, consumed) = MessageHeader::decode_with_len(input)?;
    let ciphertext = input
        .get(consumed..)
        .ok_or(CryptoError::Decode("message envelope missing ciphertext"))?;
    Ok((header, ciphertext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_header_and_ciphertext_pair() {
        let header = MessageHeader {
            dh_public: [7u8; 32],
            previous_chain_length: 2,
            message_number: 40,
        };
        let ciphertext = b"pretend this is nonce+ciphertext bytes";

        let envelope = encode(&header, ciphertext);
        let (decoded_header, decoded_ciphertext) = decode(&envelope).unwrap();

        assert_eq!(decoded_header, header);
        assert_eq!(decoded_ciphertext, ciphertext);
    }

    #[test]
    fn rejects_a_truncated_envelope() {
        let header = MessageHeader {
            dh_public: [1u8; 32],
            previous_chain_length: 0,
            message_number: 0,
        };
        let mut envelope = encode(&header, b"ciphertext");
        envelope.truncate(5);
        assert!(decode(&envelope).is_err());
    }
}
