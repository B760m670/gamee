//! Base64url (no padding) — used to embed binary payloads (QR contact
//! bundles) as text, since QR codes and URL schemes need to move through
//! text-only channels.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use crate::error::{CryptoError, Result};

pub fn encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

pub fn decode(text: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| CryptoError::Decode("invalid base64url"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_arbitrary_bytes() {
        let data = b"\x00\x01\xffSpiritChat\x7f";
        let encoded = encode(data);
        assert!(!encoded.contains('='), "no padding characters expected");
        assert_eq!(decode(&encoded).unwrap(), data);
    }

    #[test]
    fn rejects_invalid_text() {
        assert!(decode("not valid base64url!!").is_err());
    }
}
