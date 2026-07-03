//! Namespacing for `@username` claims in the same public DHT the address
//! rendezvous mechanism uses (see `rendezvous.rs`) — a username claim is
//! just another kind of record, published/looked-up the same way, under
//! its own key prefix so it can never collide with an address record or
//! another application's unrelated use of this shared DHT.
//!
//! What a DHT genuinely cannot do, with no server or blockchain backing
//! it, is arbitrate *who claimed a name first*. Two different identities
//! publishing a claim for the same username will each see their own
//! record until whichever one last (re-)published happens to be what a
//! given lookup's queried nodes are currently holding — there is no global
//! ordering. This module only handles turning a username into a
//! consistent DHT key; the actual claim contents (and therefore, whether a
//! claim can be trusted at all) are entirely the app layer's concern —
//! this crate stays as cryptography-agnostic here as it does for blobs and
//! envelopes.

use libp2p::kad::RecordKey;

const KEY_PREFIX: &[u8] = b"/spiritchat/username/1/";

/// Case-insensitive: `Alice` and `alice` resolve to the same record, the
/// same way most systems that have usernames at all treat them, so two
/// people can't each believe they hold a name that only differs by case.
pub fn normalize(username: &str) -> String {
    username.trim().to_lowercase()
}

pub fn record_key_for(username: &str) -> RecordKey {
    let mut bytes = KEY_PREFIX.to_vec();
    bytes.extend_from_slice(normalize(username).as_bytes());
    RecordKey::new(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_are_deterministic() {
        assert_eq!(record_key_for("alice"), record_key_for("alice"));
    }

    #[test]
    fn record_keys_are_case_insensitive() {
        assert_eq!(record_key_for("Alice"), record_key_for("alice"));
        assert_eq!(record_key_for("ALICE"), record_key_for("alice"));
    }

    #[test]
    fn record_keys_ignore_surrounding_whitespace() {
        assert_eq!(record_key_for("  alice  "), record_key_for("alice"));
    }

    #[test]
    fn record_keys_differ_between_usernames() {
        assert_ne!(record_key_for("alice"), record_key_for("bob"));
    }
}
