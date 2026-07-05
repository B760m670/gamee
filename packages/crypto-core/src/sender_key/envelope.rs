//! Wire framing for a single Sender Key message: the [`SenderKeyHeader`]
//! and its signature+ciphertext travel together as one blob, mirroring
//! [`crate::ratchet::envelope`]'s own reasoning exactly — one `Vec<u8>`
//! for a transport to hand off, not two values to keep in lockstep.

use crate::error::{CryptoError, Result};

use super::header::SenderKeyHeader;

/// Concatenates an encoded header with its signature+ciphertext. The
/// header is self-delimiting (see
/// [`SenderKeyHeader::decode_with_len`]), so no extra length prefix is
/// needed.
pub fn encode(header: &SenderKeyHeader, signed_ciphertext: &[u8]) -> Vec<u8> {
    let mut out = header.encode();
    out.extend_from_slice(signed_ciphertext);
    out
}

/// Splits a wire message back into its header and signature+ciphertext.
pub fn decode(input: &[u8]) -> Result<(SenderKeyHeader, &[u8])> {
    let (header, consumed) = SenderKeyHeader::decode_with_len(input)?;
    let signed_ciphertext = input.get(consumed..).ok_or(CryptoError::Decode("sender key envelope missing ciphertext"))?;
    Ok((header, signed_ciphertext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_header_and_signed_ciphertext_pair() {
        let header = SenderKeyHeader { iteration: 40 };
        let signed_ciphertext = b"pretend this is signature+nonce+ciphertext bytes";

        let envelope = encode(&header, signed_ciphertext);
        let (decoded_header, decoded_signed_ciphertext) = decode(&envelope).unwrap();

        assert_eq!(decoded_header, header);
        assert_eq!(decoded_signed_ciphertext, signed_ciphertext);
    }

    #[test]
    fn rejects_a_truncated_envelope() {
        let header = SenderKeyHeader { iteration: 0 };
        let mut envelope = encode(&header, b"signature+ciphertext");
        envelope.truncate(0);
        assert!(decode(&envelope).is_err());
    }
}
