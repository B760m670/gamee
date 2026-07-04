//! The message a group member sends — pairwise, through their already-
//! established [`crate::ratchet::DoubleRatchet`] session with each other
//! member, exactly like an ordinary 1:1 chat message — to hand out their
//! current Sender Key chain state. This is the entire "key agreement" a
//! Sender Key scheme needs: no group-wide DH tree, just N-1 ordinary
//! pairwise sends of this payload (one per other member), reusing 1:1
//! infrastructure this crate already has rather than inventing a new
//! transport-level concept. A member who joins mid-conversation only
//! receives the chain's *current* position — by design, not a bug: it can
//! never derive an earlier message's key from a later chain state, so it
//! never gains retroactive access to history it never saw distributed.

use ed25519_dalek::VerifyingKey;

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SenderKeyDistribution {
    pub chain_key: [u8; 32],
    pub iteration: u32,
    pub signing_public_key: [u8; 32],
}

impl SenderKeyDistribution {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 5 + 32);
        out.extend_from_slice(&self.chain_key);
        varint_encode(self.iteration as u64, &mut out);
        out.extend_from_slice(&self.signing_public_key);
        out
    }

    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.len() < 32 {
            return Err(CryptoError::Decode("sender key distribution shorter than a chain key"));
        }
        let chain_key: [u8; 32] = input[..32].try_into().unwrap();
        let (iteration, consumed) = varint_decode(&input[32..])?;
        let rest = &input[32 + consumed..];
        if rest.len() != 32 {
            return Err(CryptoError::Decode("sender key distribution has a malformed signing public key"));
        }
        // Parsed eagerly (not deferred to first use) purely to fail fast
        // on the wrong-length case above before it can reach a signing
        // key field at all — `ed25519_dalek::VerifyingKey::from_bytes`
        // itself accepts any 32 bytes (it doesn't reject non-canonical
        // points), so this is not a meaningful cryptographic check; a
        // truly malformed signing key only ever surfaces later, as an
        // ordinary signature-verification failure on the first message.
        VerifyingKey::from_bytes(rest.try_into().unwrap())
            .map_err(|_| CryptoError::Decode("sender key distribution has an invalid Ed25519 public key"))?;
        Ok(Self {
            chain_key,
            iteration: iteration as u32,
            signing_public_key: rest.try_into().unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SenderKeyDistribution {
        SenderKeyDistribution {
            chain_key: [7u8; 32],
            iteration: 42,
            signing_public_key: crate::identity::IdentityKeyPair::generate(&mut rand_core::OsRng)
                .public_key()
                .to_bytes(),
        }
    }

    #[test]
    fn round_trips_through_bytes() {
        let distribution = sample();
        let decoded = SenderKeyDistribution::decode(&distribution.encode()).unwrap();
        assert_eq!(decoded, distribution);
    }

    #[test]
    fn rejects_truncated_bytes() {
        let mut bytes = sample().encode();
        bytes.truncate(10);
        assert!(SenderKeyDistribution::decode(&bytes).is_err());
    }

    #[test]
    fn rejects_a_signing_public_key_of_the_wrong_length() {
        let mut bytes = sample().encode();
        bytes.push(0); // one byte too many trailing the signing public key
        assert!(SenderKeyDistribution::decode(&bytes).is_err());
    }
}
