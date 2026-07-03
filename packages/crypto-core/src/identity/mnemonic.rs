//! BIP39 mnemonic-derived identity recovery.
//!
//! There is no server to hold a "reset password" link, and this project's
//! whole point is that there never will be — so the only way to survive
//! losing a device is a secret the person themselves holds, the same way
//! every cryptocurrency wallet does it. The mnemonic isn't an extra backup
//! *alongside* the identity key; it *is* the identity key, spelled out as
//! words instead of bytes, via the same derivation wallets use to turn a
//! seed phrase into an Ed25519 signing key: SLIP-0010's Ed25519 master-key
//! derivation, `HMAC-SHA512(key = "ed25519 seed", data = bip39_seed)`,
//! keeping the left 32 bytes. Plain truncation of the raw BIP39 seed would
//! also produce *a* 32-byte value, but not the standards-defined one other
//! wallets/tools would derive from the same words — SLIP-0010 is what makes
//! "write down these words" an interoperable, independently-verifiable
//! recovery mechanism instead of a bespoke one only this app understands.
//!
//! Deliberately *not* recoverable this way: the agreement key and prekeys.
//! Deriving those from the same phrase too would mean anyone who ever
//! recorded a past agreement key (there's no way to *prove* they didn't)
//! could reconstruct it again from the recovered phrase, which defeats the
//! forward secrecy the Double Ratchet is there to provide. Recovering an
//! account gets back the same identity (same fingerprint, same PeerId) with
//! fresh session state — existing contacts will see a new agreement key and
//! need to re-handshake, the same way losing a Signal-linked device does.

use bip39::Mnemonic;
use hmac::{Hmac, Mac};
use rand_core::CryptoRngCore;
use sha2::Sha512;
use zeroize::Zeroize;

use crate::error::{CryptoError, Result};

const SLIP10_ED25519_SEED_KEY: &[u8] = b"ed25519 seed";

/// 128 bits of entropy -> 12 words. The same default most wallets (MetaMask,
/// Trust Wallet, ...) ship: computationally infeasible to guess, short
/// enough that a person can actually write it down and re-type it correctly.
const ENTROPY_BYTES: usize = 16;

/// A BIP39 recovery phrase. No passphrase ("25th word") support — it adds a
/// second secret that's just as catastrophic to forget as the first one,
/// for a security margin (protecting against a physically stolen phrase)
/// this project doesn't otherwise ask users to reason about.
pub struct RecoveryPhrase(Mnemonic);

impl RecoveryPhrase {
    /// Generates a brand-new phrase. Call this exactly once per account,
    /// same as `IdentityKeyPair::generate` — this *replaces* that as the
    /// source of the identity seed, not an addition to it.
    pub fn generate(rng: &mut impl CryptoRngCore) -> Self {
        let mut entropy = [0u8; ENTROPY_BYTES];
        rng.fill_bytes(&mut entropy);
        let mnemonic =
            Mnemonic::from_entropy(&entropy).expect("16 bytes is always valid BIP39 entropy");
        entropy.zeroize();
        Self(mnemonic)
    }

    /// Parses and checksum-validates a phrase the user typed back in (e.g.
    /// during account recovery on a fresh install). A typo in any single
    /// word is caught here, before it can silently derive the wrong
    /// identity.
    pub fn parse(phrase: &str) -> Result<Self> {
        Mnemonic::parse(phrase)
            .map(Self)
            .map_err(|err| CryptoError::InvalidRecoveryPhrase(err.to_string()))
    }

    /// The words, space-separated, to display once at account creation (and
    /// nowhere else this crate is concerned with — whether/where the app
    /// persists them afterward is its call, not this crate's).
    pub fn words(&self) -> String {
        self.0.words().collect::<Vec<_>>().join(" ")
    }

    /// Derives the 32-byte identity seed this phrase corresponds to — the
    /// same seed format `IdentityKeyPair::from_bytes` and
    /// `spiritchat_p2p_core::keypair_from_seed` already take, so a recovered
    /// account's fingerprint and PeerId are identical to the original
    /// device's.
    pub fn derive_identity_seed(&self) -> [u8; 32] {
        let mut bip39_seed = self.0.to_seed("");
        let seed = slip10_ed25519_master_key(&bip39_seed);
        bip39_seed.zeroize();
        seed
    }
}

/// SLIP-0010's Ed25519 master-key derivation: `HMAC-SHA512(key = "ed25519
/// seed", data = seed)`, keeping the left 32 bytes as the private key (the
/// right 32 bytes are the chain code, used for further hardened child
/// derivation — irrelevant here since this crate only ever derives the
/// master key itself, not a derivation path under it). Split out from
/// `derive_identity_seed` so it can be checked against SLIP-0010's own
/// published test vectors independent of BIP39, in `tests::slip10_test_vectors`.
fn slip10_ed25519_master_key(seed: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha512>::new_from_slice(SLIP10_ED25519_SEED_KEY)
        .expect("HMAC-SHA512 accepts a key of any length");
    mac.update(seed);
    let derived = mac.finalize().into_bytes();

    let mut key = [0u8; 32];
    key.copy_from_slice(&derived[..32]);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(1)
    }

    #[test]
    fn generates_twelve_words() {
        let phrase = RecoveryPhrase::generate(&mut test_rng());
        assert_eq!(phrase.words().split_whitespace().count(), 12);
    }

    #[test]
    fn a_generated_phrase_round_trips_through_parsing() {
        let phrase = RecoveryPhrase::generate(&mut test_rng());
        let words = phrase.words();
        let parsed = RecoveryPhrase::parse(&words).expect("a freshly generated phrase must parse");
        assert_eq!(phrase.derive_identity_seed(), parsed.derive_identity_seed());
    }

    #[test]
    fn the_same_phrase_always_derives_the_same_seed() {
        let phrase = RecoveryPhrase::generate(&mut test_rng());
        let words = phrase.words();
        let a = RecoveryPhrase::parse(&words).unwrap().derive_identity_seed();
        let b = RecoveryPhrase::parse(&words).unwrap().derive_identity_seed();
        assert_eq!(a, b);
    }

    #[test]
    fn different_phrases_derive_different_seeds() {
        let a = RecoveryPhrase::generate(&mut test_rng()).derive_identity_seed();
        let b = RecoveryPhrase::generate(&mut ChaCha20Rng::seed_from_u64(2)).derive_identity_seed();
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_a_phrase_with_a_typo() {
        let phrase = RecoveryPhrase::generate(&mut test_rng());
        let original = phrase.words();
        let mut words: Vec<&str> = original.split_whitespace().collect();
        words[0] = "zzzznotarealbip39word";
        let mangled = words.join(" ");
        assert!(RecoveryPhrase::parse(&mangled).is_err());
    }

    #[test]
    fn rejects_a_phrase_with_a_corrupted_checksum() {
        // Swap the last two words: still 12 valid dictionary words, but the
        // checksum bits (which depend on word order) will almost certainly
        // no longer validate.
        let phrase = RecoveryPhrase::generate(&mut test_rng());
        let original = phrase.words();
        let mut words: Vec<&str> = original.split_whitespace().collect();
        words.swap(0, 11);
        let reordered = words.join(" ");
        assert!(RecoveryPhrase::parse(&reordered).is_err());
    }

    /// The master-key ("m") cases from SLIP-0010's own published Ed25519
    /// test vectors
    /// (<https://github.com/satoshilabs/slips/blob/master/slip-0010.md>),
    /// cross-checked against the independent `slip10_ed25519` crate's test
    /// suite (which asserts the same two outputs for the same two seeds).
    /// This is what actually verifies `slip10_ed25519_master_key` implements
    /// the standard, not just that it's internally self-consistent — every
    /// other test in this file would still pass even if the derivation used
    /// the wrong HMAC key or byte order, as long as it did so consistently.
    #[test]
    fn slip10_test_vectors() {
        fn hex_decode(s: &str) -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        }
        fn hex_encode(bytes: &[u8]) -> String {
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }

        let case1_seed = hex_decode("000102030405060708090a0b0c0d0e0f");
        assert_eq!(
            hex_encode(&slip10_ed25519_master_key(&case1_seed)),
            "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7"
        );

        let case2_seed = hex_decode(
            "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a\
             29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b4845\
             42",
        );
        assert_eq!(
            hex_encode(&slip10_ed25519_master_key(&case2_seed)),
            "171cb88b1b3c1db25add599712e36245d75bc65a1a5c9e18d76f9f2b1eab4012"
        );
    }

    #[test]
    fn the_derived_seed_is_a_valid_identity_key() {
        use crate::identity::IdentityKeyPair;

        let phrase = RecoveryPhrase::generate(&mut test_rng());
        let seed = phrase.derive_identity_seed();
        let identity = IdentityKeyPair::from_bytes(&seed).expect("a derived seed must be usable");

        // Re-deriving from the same words must reconstruct the same key,
        // not just *a* key.
        let recovered_seed = RecoveryPhrase::parse(&phrase.words()).unwrap().derive_identity_seed();
        let recovered = IdentityKeyPair::from_bytes(&recovered_seed).unwrap();
        assert_eq!(identity.public_key(), recovered.public_key());
    }
}
