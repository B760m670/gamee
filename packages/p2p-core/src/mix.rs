//! Anonymous multi-hop routing for mailbox deposits/retrievals, built on
//! the **Sphinx packet format** — not a homemade design. `sphinx-packet`
//! (Apache-2.0, from the Nym project) is a small, standalone
//! implementation of the format underlying Nym, a live, funded anonymity
//! network; this crate depends only on the packet-format library itself,
//! never connecting to Nym's own network. Every transitive dependency
//! (`x25519-dalek`, `curve25519-dalek`, `sha2`, `rand_core`) is already a
//! dependency of `spiritchat-crypto-core` elsewhere in this workspace and
//! already proven to cross-compile for this project's iOS/Android targets
//! in CI — the same category of crate this project already trusts for its
//! X3DH/Double Ratchet implementation, not a new class of risk.
//!
//! What a Sphinx packet buys, precisely: each hop can decrypt only its own
//! routing layer (who handed it this packet, who to forward to next, how
//! long to hold it) — never the full path, never whether it's holding the
//! first or last hop. `sphinx_packet::header::delays` generates
//! Loopix-style Poisson (exponentially distributed) per-hop delays
//! natively; layering that on top of bare onion-peeling is what turns
//! "hides the path" into "also resists timing correlation," the second
//! half of Loopix's actual published design (mixing) beyond Sphinx's own
//! (routing).
//!
//! This module only builds/peels packets — pure logic, no networking, no
//! knowledge of `PeerId`/the swarm. Wiring a peeled `ForwardHop` to an
//! actual `SendEnvelope`-style dial-and-deliver, and a `FinalHop` into
//! `mailbox.rs`'s (soon to be tag-addressed) storage, is Phase 2's job.

use hkdf::Hkdf;
use sha2::Sha256;
use sphinx_packet::header::delays::{self, Delay};
use sphinx_packet::route::{Destination, DestinationAddressBytes, Node, NodeAddressBytes, SURBIdentifier};
use sphinx_packet::{ProcessedPacket, ProcessedPacketData, SphinxPacket};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::error::{P2pError, Result};

/// The Sphinx format's own hard cap on how many hops a single packet's
/// header can describe — not a policy choice this crate makes.
pub const MAX_PATH_LENGTH: usize = sphinx_packet::constants::MAX_PATH_LENGTH;

fn to_mix_err(err: impl std::fmt::Display) -> P2pError {
    P2pError::Mix(err.to_string())
}

/// A mix hop this node knows how to route through — its outward-facing
/// 32-byte address (see `node_address_for`) and its Sphinx routing public
/// key (distinct from, though derived the same way as, its libp2p
/// identity key — see Phase 2 for how these get published/discovered).
pub struct MixHop {
    pub address: NodeAddressBytes,
    pub public_key: PublicKey,
}

/// Derives the 32-byte address a `PeerId` is routed under inside a Sphinx
/// packet — the packet format's own address field is fixed-size and
/// doesn't fit libp2p's (longer, multihash-wrapped) `PeerId` encoding
/// directly, so this is a stable hash of it instead. Never the identity
/// itself: a hop only ever learns "forward to whoever answers to this
/// hash," resolved back to a real dialable peer via Phase 2's mix-relay
/// directory, not by inverting the hash.
pub fn node_address_for(peer_id_bytes: &[u8]) -> NodeAddressBytes {
    use sha2::Digest;
    let digest = Sha256::digest(peer_id_bytes);
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    NodeAddressBytes::from_bytes(bytes)
}

/// Derives this node's mix routing keypair from the same 32-byte seed its
/// libp2p identity already comes from (`identity::keypair_from_seed`) —
/// domain-separated via HKDF so one raw seed produces two cryptographically
/// independent keys for two different jobs (the signing/noise-handshake
/// identity vs. Sphinx's own per-hop Diffie-Hellman), never reusing raw key
/// material across purposes. Deterministic on purpose: this node needs to
/// reconstruct the same routing secret on every restart without a separate
/// keyfile, the same way its libp2p identity already does.
pub fn routing_keypair_from_seed(identity_seed: &[u8; 32]) -> (StaticSecret, PublicKey) {
    let hk = Hkdf::<Sha256>::new(None, identity_seed);
    let mut scalar = [0u8; 32];
    hk.expand(b"spiritchat-mix-routing-key-v1", &mut scalar)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    let secret = StaticSecret::from(scalar);
    let public = PublicKey::from(&secret);
    (secret, public)
}

/// Builds a Sphinx packet carrying `message` through `path` (in order),
/// with independently-sampled Poisson delays per hop (Loopix's mixing
/// model — see the module doc comment) averaging `average_hop_delay`,
/// terminating at `destination`. `path.len()` must be within
/// `MAX_PATH_LENGTH`; validated here rather than left to the crate to
/// reject less legibly.
pub fn build_packet(
    message: &[u8],
    path: &[MixHop],
    destination_address: DestinationAddressBytes,
    destination_identifier: SURBIdentifier,
    average_hop_delay: std::time::Duration,
) -> Result<SphinxPacket> {
    if path.is_empty() || path.len() > MAX_PATH_LENGTH {
        return Err(P2pError::Mix(format!(
            "mix path must have between 1 and {MAX_PATH_LENGTH} hops, got {}",
            path.len()
        )));
    }

    let route: Vec<Node> = path.iter().map(|hop| Node::new(hop.address, hop.public_key)).collect();
    let delays = delays::generate_from_average_duration(path.len(), average_hop_delay);
    let destination = Destination::new(destination_address, destination_identifier);

    SphinxPacket::new(message.to_vec(), &route, &destination, &delays).map_err(to_mix_err)
}

/// Prefixes the plaintext payload of loop/cover traffic — Sphinx packets
/// this node (or a peer doing the same thing) generated purely to shape
/// traffic timing/volume, never a real deposit or query. Only the packet's
/// final hop ever sees a payload at all (an intermediate relay only ever
/// sees a `Forward` outcome, never plaintext), so this marker never leaks
/// to anyone the dummy packet wasn't already addressed to — which, for
/// both loop and cover traffic, is always either this node itself or
/// exactly the peer generating its own dummy traffic the same way, so
/// seeing it is never surprising or evidence that a real message was
/// suppressed.
const DUMMY_PAYLOAD_MARKER: &[u8] = b"spiritchat-mix-dummy-v1";

/// Whether a peeled final-hop payload is loop/cover traffic rather than a
/// real deposit or query — the receiving end's cue to discard it silently
/// instead of surfacing `P2pEvent::MixPacketArrived`.
pub fn is_dummy_payload(payload: &[u8]) -> bool {
    payload.starts_with(DUMMY_PAYLOAD_MARKER)
}

/// Builds a Sphinx packet carrying nothing but `DUMMY_PAYLOAD_MARKER` —
/// bitwise indistinguishable from a real deposit/query packet to anyone
/// but the final hop that decrypts it, which is the whole point: loop and
/// cover traffic must cost an outside observer nothing to rule out.
pub fn build_dummy_packet(
    path: &[MixHop],
    destination_address: DestinationAddressBytes,
    destination_identifier: SURBIdentifier,
    average_hop_delay: std::time::Duration,
) -> Result<SphinxPacket> {
    build_packet(DUMMY_PAYLOAD_MARKER, path, destination_address, destination_identifier, average_hop_delay)
}

/// What peeling one layer off an incoming packet, at this node, produces.
pub enum PeelOutcome {
    /// Not the final hop — forward `next_hop_packet` to whoever answers to
    /// `next_hop_address`, but only after `delay` (Loopix mixing: this is
    /// exactly what breaks the arrival/departure timing correlation an
    /// observer would otherwise be able to make).
    Forward { next_hop_packet: SphinxPacket, next_hop_address: NodeAddressBytes, delay: Delay },
    /// This node is the final hop — `payload` is the original deposit/
    /// retrieval-request bytes this packet was built to carry.
    Final { destination_address: DestinationAddressBytes, identifier: SURBIdentifier, payload: Vec<u8> },
}

/// Peels exactly one Sphinx layer using this node's own mix routing secret
/// key (distinct from its libp2p identity key). Never inspects, and
/// cannot recover, anything about hops before or after the immediate
/// neighbors this layer reveals.
pub fn peel(packet: SphinxPacket, node_secret_key: &StaticSecret) -> Result<PeelOutcome> {
    let processed: ProcessedPacket = packet.process(node_secret_key).map_err(to_mix_err)?;
    match processed.data {
        ProcessedPacketData::ForwardHop { next_hop_packet, next_hop_address, delay } => {
            Ok(PeelOutcome::Forward { next_hop_packet, next_hop_address, delay })
        }
        ProcessedPacketData::FinalHop { destination, identifier, payload } => Ok(PeelOutcome::Final {
            destination_address: destination,
            identifier,
            payload: payload.recover_plaintext().map_err(to_mix_err)?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    struct TestHop {
        secret: StaticSecret,
        hop: MixHop,
    }

    fn generate_hop(address_seed: u8) -> TestHop {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public_key = PublicKey::from(&secret);
        let address = NodeAddressBytes::from_bytes([address_seed; 32]);
        TestHop { secret, hop: MixHop { address, public_key } }
    }

    #[test]
    fn a_message_survives_being_peeled_through_every_hop_of_a_multi_hop_path() {
        let hops = [generate_hop(1), generate_hop(2), generate_hop(3)];
        let path: Vec<MixHop> = hops
            .iter()
            .map(|h| MixHop { address: h.hop.address, public_key: h.hop.public_key })
            .collect();

        let destination_address = DestinationAddressBytes::from_bytes([9u8; 32]);
        let identifier: SURBIdentifier = [7u8; 16];
        let message = b"a message no single hop should be able to read or fully trace";

        let packet = build_packet(
            message,
            &path,
            destination_address,
            identifier,
            std::time::Duration::from_millis(50),
        )
        .unwrap();

        // Hop 1: must forward, not terminate.
        let after_hop1 = peel(packet, &hops[0].secret).unwrap();
        let PeelOutcome::Forward { next_hop_packet, next_hop_address, .. } = after_hop1 else {
            panic!("the first of three hops must be a Forward outcome, not Final");
        };
        assert_eq!(next_hop_address, hops[1].hop.address);

        // Hop 2: must also forward.
        let after_hop2 = peel(next_hop_packet, &hops[1].secret).unwrap();
        let PeelOutcome::Forward { next_hop_packet, next_hop_address, .. } = after_hop2 else {
            panic!("the second of three hops must be a Forward outcome, not Final");
        };
        assert_eq!(next_hop_address, hops[2].hop.address);

        // Hop 3 (final): must terminate with the original message intact.
        let after_hop3 = peel(next_hop_packet, &hops[2].secret).unwrap();
        let PeelOutcome::Final { destination_address: got_destination, identifier: got_identifier, payload } =
            after_hop3
        else {
            panic!("the last hop must be a Final outcome");
        };
        assert_eq!(got_destination, destination_address);
        assert_eq!(got_identifier, identifier);
        assert_eq!(&payload[..message.len()], message);
    }

    #[test]
    fn a_path_longer_than_the_format_allows_is_rejected_before_building_anything() {
        let hops: Vec<TestHop> = (0..(MAX_PATH_LENGTH as u8 + 1)).map(generate_hop).collect();
        let path: Vec<MixHop> = hops.iter().map(|h| MixHop { address: h.hop.address, public_key: h.hop.public_key }).collect();

        let result = build_packet(
            b"too many hops",
            &path,
            DestinationAddressBytes::from_bytes([0u8; 32]),
            [0u8; 16],
            std::time::Duration::from_millis(1),
        );
        match result {
            Err(P2pError::Mix(_)) => {}
            Err(other) => panic!("expected a Mix error, got a different P2pError variant: {other}"),
            Ok(_) => panic!("a path longer than MAX_PATH_LENGTH must be rejected"),
        }
    }

    #[test]
    fn routing_keys_are_deterministic_from_seed_but_differ_from_a_different_seed() {
        let (secret_a, public_a) = routing_keypair_from_seed(&[1u8; 32]);
        let (_secret_a_again, public_a_again) = routing_keypair_from_seed(&[1u8; 32]);
        let (_secret_b, public_b) = routing_keypair_from_seed(&[2u8; 32]);

        assert_eq!(public_a.as_bytes(), public_a_again.as_bytes());
        assert_ne!(public_a.as_bytes(), public_b.as_bytes());
        // The derived secret must actually match the derived public key —
        // not just be *some* deterministic value.
        assert_eq!(PublicKey::from(&secret_a).as_bytes(), public_a.as_bytes());
    }

    #[test]
    fn a_dummy_packet_is_recognized_as_dummy_once_peeled() {
        let hop = generate_hop(1);
        let path = vec![MixHop { address: hop.hop.address, public_key: hop.hop.public_key }];

        let packet = build_dummy_packet(
            &path,
            DestinationAddressBytes::from_bytes([0u8; 32]),
            [0u8; 16],
            std::time::Duration::from_millis(1),
        )
        .unwrap();

        let PeelOutcome::Final { payload, .. } = peel(packet, &hop.secret).unwrap() else {
            panic!("a single-hop dummy packet must peel to a Final outcome");
        };
        assert!(is_dummy_payload(&payload));
    }

    #[test]
    fn a_real_message_is_never_mistaken_for_a_dummy() {
        assert!(!is_dummy_payload(b"a message no single hop should be able to read or fully trace"));
        assert!(!is_dummy_payload(b""));
    }

    #[test]
    fn a_packet_round_trips_through_its_own_byte_encoding() {
        let hops = [generate_hop(1)];
        let path: Vec<MixHop> = hops.iter().map(|h| MixHop { address: h.hop.address, public_key: h.hop.public_key }).collect();
        let packet = build_packet(
            b"single hop",
            &path,
            DestinationAddressBytes::from_bytes([5u8; 32]),
            [1u8; 16],
            std::time::Duration::from_millis(1),
        )
        .unwrap();

        let bytes = packet.to_bytes();
        let restored = SphinxPacket::from_bytes(&bytes).unwrap();

        let PeelOutcome::Final { payload, .. } = peel(restored, &hops[0].secret).unwrap() else {
            panic!("expected the single hop to be Final");
        };
        assert_eq!(&payload[..b"single hop".len()], b"single hop");
    }
}
