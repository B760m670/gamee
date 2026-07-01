//! LEB128 unsigned varint, used to length-prefix variable-size fields in our
//! wire formats (message headers, QR payloads) without pulling in a full
//! serialization framework.

use crate::error::{CryptoError, Result};

pub fn encode(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Decodes a varint from the front of `input`, returning the value and the
/// number of bytes consumed.
pub fn decode(input: &[u8]) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for (i, &byte) in input.iter().enumerate() {
        if shift >= 64 {
            return Err(CryptoError::Decode("varint too long"));
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        shift += 7;
    }
    Err(CryptoError::Decode("truncated varint"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_small_and_large_values() {
        for value in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            encode(value, &mut buf);
            let (decoded, consumed) = decode(&buf).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(consumed, buf.len());
        }
    }

    #[test]
    fn rejects_truncated_input() {
        let mut buf = Vec::new();
        encode(u64::MAX, &mut buf);
        buf.truncate(buf.len() - 1);
        assert!(decode(&buf).is_err());
    }

    #[test]
    fn ignores_trailing_bytes_after_one_varint() {
        let mut buf = Vec::new();
        encode(42, &mut buf);
        buf.push(0xff);
        let (value, consumed) = decode(&buf).unwrap();
        assert_eq!(value, 42);
        assert_eq!(consumed, 1);
    }
}
