//! KDF_RK from the Double Ratchet spec: mixes a DH output into the root
//! key, producing both a new root key and a fresh chain key for the side
//! that just performed a DH ratchet step.

use hkdf::Hkdf;
use sha2::Sha256;

use super::chain::ChainKey;

const ROOT_KDF_INFO: &[u8] = b"SpiritChat-DR-RootKDF-v1";

pub fn derive(root_key: &[u8; 32], dh_output: &[u8; 32]) -> ([u8; 32], ChainKey) {
    let hk = Hkdf::<Sha256>::new(Some(root_key), dh_output);
    let mut okm = [0u8; 64];
    hk.expand(ROOT_KDF_INFO, &mut okm)
        .expect("64 bytes is a valid HKDF-SHA256 output length");

    let mut new_root_key = [0u8; 32];
    let mut new_chain_key = [0u8; 32];
    new_root_key.copy_from_slice(&okm[..32]);
    new_chain_key.copy_from_slice(&okm[32..]);

    (new_root_key, ChainKey::new(new_chain_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        let (rk1, ck1) = derive(&[1u8; 32], &[2u8; 32]);
        let (rk2, ck2) = derive(&[1u8; 32], &[2u8; 32]);
        assert_eq!(rk1, rk2);
        assert_eq!(ck1.as_bytes(), ck2.as_bytes());
    }

    #[test]
    fn root_key_and_chain_key_differ() {
        let (rk, ck) = derive(&[1u8; 32], &[2u8; 32]);
        assert_ne!(&rk, ck.as_bytes());
    }

    #[test]
    fn different_dh_output_changes_both_outputs() {
        let (rk1, ck1) = derive(&[1u8; 32], &[2u8; 32]);
        let (rk2, ck2) = derive(&[1u8; 32], &[3u8; 32]);
        assert_ne!(rk1, rk2);
        assert_ne!(ck1.as_bytes(), ck2.as_bytes());
    }
}
