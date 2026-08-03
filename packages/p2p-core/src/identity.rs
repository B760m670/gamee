//! Turns a 32-byte Ed25519 seed into a libp2p network identity. Deliberately
//! the same seed format `spiritchat_crypto_core::identity::IdentityKeyPair`
//! already uses (`ed25519_from_bytes` takes exactly a 32-byte scalar), so
//! the same identity key doubles as both the X3DH signing identity and the
//! libp2p `PeerId` — one key, not two to generate and persist separately.

use libp2p::identity::{ed25519, Keypair, PublicKey};
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

/// The raw 32-byte Ed25519 *public* key for a seed — the public half of the
/// identity `keypair_from_seed` builds.
///
/// Exists so a headless node (see `packages/relay-node`) can name itself in
/// data it publishes — a mined block's `miner_public_key`, for instance —
/// without depending on `libp2p` directly just to unwrap a `PublicKey`, and,
/// more importantly, without ever being tempted to reach for the seed itself:
/// the seed is the relay's private identity, and putting it in a block header
/// would publish its secret key to the whole network.
pub fn public_key_bytes_from_seed(seed: &[u8; 32]) -> Result<[u8; 32]> {
    let public = keypair_from_seed(seed)?
        .public()
        .try_into_ed25519()
        .map_err(|err| P2pError::InvalidSeed(err.to_string()))?;
    Ok(public.to_bytes())
}

/// Derives the `PeerId` a raw 32-byte Ed25519 public key would produce as a
/// libp2p identity — used to turn a username claim's public key (resolved
/// from the DHT, see `crate::username`) into something dialable, without
/// needing the claim holder's node to also publish its PeerId separately:
/// same reasoning as `peer_id_from_seed`, just starting from the public half.
pub fn peer_id_from_public_key(public_key: &[u8]) -> Result<PeerId> {
    let key = ed25519::PublicKey::try_from_bytes(public_key)
        .map_err(|err| P2pError::InvalidSeed(err.to_string()))?;
    Ok(PublicKey::from(key).to_peer_id())
}

#[cfg(test)]
mod public_key_tests {
    use super::*;

    #[test]
    fn public_key_bytes_match_the_keypairs_own_public_half() {
        let seed = [11u8; 32];
        let expected = keypair_from_seed(&seed).unwrap().public().try_into_ed25519().unwrap().to_bytes();
        assert_eq!(public_key_bytes_from_seed(&seed).unwrap(), expected);
    }

    #[test]
    fn public_key_bytes_are_never_the_seed_itself() {
        // Guards the one mistake that would be catastrophic here: publishing
        // the private seed as if it were an identifier.
        let seed = [13u8; 32];
        assert_ne!(public_key_bytes_from_seed(&seed).unwrap(), seed);
    }

    #[test]
    fn matches_the_peer_id_derived_from_the_same_seed() {
        let seed = [7u8; 32];
        let from_seed = peer_id_from_seed(&seed).unwrap();
        let public_key = keypair_from_seed(&seed).unwrap().public().try_into_ed25519().unwrap().to_bytes();
        let from_public_key = peer_id_from_public_key(&public_key).unwrap();
        assert_eq!(from_seed, from_public_key);
    }
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
