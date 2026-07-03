//! The symmetric-key ratchet: a one-way KDF chain that turns a chain key
//! into an endless stream of one-time message keys, each step destroying
//! the ability to recompute the previous one (forward secrecy within a
//! chain, independent of the DH ratchet).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::ZeroizeOnDrop;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, ZeroizeOnDrop)]
pub struct ChainKey([u8; 32]);

#[derive(Clone, ZeroizeOnDrop)]
pub struct MessageKey(pub(crate) [u8; 32]);

impl ChainKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Advances the chain by one step, returning the message key for this
    /// step and the next chain key. The single-byte constants (0x01, 0x02)
    /// are fixed, public domain-separation labels from the Double Ratchet
    /// spec — they only need to differ from each other, not be secret.
    pub fn ratchet(&self) -> (MessageKey, ChainKey) {
        let message_key = hmac(&self.0, &[0x01]);
        let next_chain_key = hmac(&self.0, &[0x02]);
        (MessageKey(message_key), ChainKey(next_chain_key))
    }
}

impl MessageKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn hmac(key: &[u8; 32], input: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(input);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratcheting_is_deterministic() {
        let ck = ChainKey::new([7u8; 32]);
        let (mk1, next1) = ck.ratchet();
        let (mk2, next2) = ck.ratchet();
        assert_eq!(mk1.as_bytes(), mk2.as_bytes());
        assert_eq!(next1.as_bytes(), next2.as_bytes());
    }

    #[test]
    fn each_step_produces_a_different_message_key() {
        let ck = ChainKey::new([1u8; 32]);
        let (mk1, ck1) = ck.ratchet();
        let (mk2, ck2) = ck1.ratchet();
        let (mk3, _ck3) = ck2.ratchet();
        assert_ne!(mk1.as_bytes(), mk2.as_bytes());
        assert_ne!(mk2.as_bytes(), mk3.as_bytes());
        assert_ne!(mk1.as_bytes(), mk3.as_bytes());
    }

    #[test]
    fn message_key_and_next_chain_key_differ() {
        // A bug that returned the same bytes for both outputs would let an
        // attacker who learns one message key derive the whole rest of the
        // chain, defeating forward secrecy.
        let ck = ChainKey::new([1u8; 32]);
        let (mk, next) = ck.ratchet();
        assert_ne!(mk.as_bytes(), next.as_bytes());
    }
}
