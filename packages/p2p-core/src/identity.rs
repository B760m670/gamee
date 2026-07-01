//! Turns a 32-byte Ed25519 seed into a libp2p network identity. Deliberately
//! the same seed format `spiritchat_crypto_core::identity::IdentityKeyPair`
//! already uses (`ed25519_from_bytes` takes exactly a 32-byte scalar), so
//! the same identity key doubles as both the X3DH signing identity and the
//! libp2p `PeerId` — one key, not two to generate and persist separately.

use libp2p::identity::Keypair;
use libp2p::PeerId;

use crate::error::{P2pError, Result};

pub fn keypair_from_seed(seed: &[u8; 32]) -> Result<Keypair> {
    let mut bytes = *seed;
    Keypair::ed25519_from_bytes(&mut bytes)
        .map_err(|err| P2pError::InvalidSeed(err.to_string()))
}

pub fn peer_id_from_seed(seed: &[u8; 32]) -> Result<PeerId> {
    Ok(keypair_from_seed(seed)?.public().to_peer_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_always_produces_the_same_peer_id() {
        let seed = [9u8; 32];
        let a = peer_id_from_seed(&seed).unwrap();
        let b = peer_id_from_seed(&seed).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_produce_different_peer_ids() {
        let a = peer_id_from_seed(&[1u8; 32]).unwrap();
        let b = peer_id_from_seed(&[2u8; 32]).unwrap();
        assert_ne!(a, b);
    }
}
