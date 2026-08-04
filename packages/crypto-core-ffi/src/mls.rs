//! UniFFI bridge for `spiritchat_mls_core` — TreeKEM group messaging,
//! exposed to Swift/Kotlin. Mirrors the shape of the Sender Keys bridge
//! (`crate::sender_key`): stateful objects wrap a `Mutex` (commits and
//! message encryption both advance internal state through `&self`), and
//! everything that crosses the boundary or persists does so as bytes.
//!
//! The group's member identity is the app's own Ed25519 identity key: the
//! caller passes `identity_seed` (the same secret bytes
//! `FfiIdentity::secret_bytes` already exposes), so a member's place in
//! the tree is bound to the exact identity everything else in SpiritChat
//! trusts. Leaf keys (the X25519 keys the ratchet tree ratchets) are
//! separate, generated per membership and persisted by the caller until a
//! join consumes them.

use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use spiritchat_mls_core::group::{commit_id, Commit, GroupState, Proposal, Welcome};
use spiritchat_mls_core::member::KeyPackage;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::error::{FfiError, FfiResult};

fn mls_err(reason: &str) -> FfiError {
    FfiError::Crypto { reason: format!("mls: {reason}") }
}

fn seed32(bytes: &[u8], what: &str) -> FfiResult<[u8; 32]> {
    bytes.try_into().map_err(|_| mls_err(&format!("{what} must be 32 bytes, got {}", bytes.len())))
}

fn signing_key(identity_seed: &[u8]) -> FfiResult<SigningKey> {
    Ok(SigningKey::from_bytes(&seed32(identity_seed, "identity seed")?))
}

/// A fresh X25519 leaf keypair for a future group membership. The caller
/// persists `secret` (as securely as any key), publishes the key package
/// built from it (`mls_key_package`), and hands the secret back to
/// `FfiMlsGroup::join` once the committer's Welcome arrives.
#[derive(uniffi::Record)]
pub struct FfiMlsLeafKey {
    pub secret: Vec<u8>,
    pub public_key: Vec<u8>,
}

/// Generates a leaf keypair.
#[uniffi::export]
pub fn mls_generate_leaf_key() -> FfiMlsLeafKey {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    FfiMlsLeafKey { secret: secret.to_bytes().to_vec(), public_key: public.to_bytes().to_vec() }
}

/// Builds and self-signs a key package (a signed offer to occupy a leaf)
/// from this identity and a leaf public key — what a would-be member
/// publishes so someone already in a group can add them.
#[uniffi::export]
pub fn mls_key_package(identity_seed: Vec<u8>, leaf_public: Vec<u8>) -> FfiResult<Vec<u8>> {
    let sk = signing_key(&identity_seed)?;
    let leaf = PublicKey::from(seed32(&leaf_public, "leaf public")?);
    let kp = KeyPackage::create(&sk, leaf);
    Ok(bincode::serialize(&kp).map_err(|_| mls_err("key package serialization failed"))?)
}

/// A commit's stable public id — Swift compares these to arbitrate two
/// concurrent commits (smallest wins its epoch; see mls-core's arbiter).
#[uniffi::export]
pub fn mls_commit_id(group_id: Vec<u8>, commit_bytes: Vec<u8>) -> FfiResult<Vec<u8>> {
    let commit = Commit::from_bytes(&commit_bytes).map_err(|_| mls_err("malformed commit"))?;
    Ok(commit_id(&group_id, &commit).to_vec())
}

/// One added member's Welcome, paired with which identity it's for.
#[derive(uniffi::Record)]
pub struct FfiMlsWelcome {
    pub member_identity: Vec<u8>,
    pub welcome_bytes: Vec<u8>,
}

/// The result of committing: the commit to broadcast, one Welcome per
/// added member, and — for arbitration — the serialized state this member
/// *would* advance to if this commit wins. The caller adopts `next_state`
/// only once the commit is known to be canonical (`commit` below does that
/// immediately; `build_commit` leaves it to the caller).
#[derive(uniffi::Record)]
pub struct FfiMlsCommitOutput {
    pub commit_bytes: Vec<u8>,
    pub welcomes: Vec<FfiMlsWelcome>,
    pub next_state: Vec<u8>,
}

/// A decrypted group message.
#[derive(uniffi::Record)]
pub struct FfiMlsMessage {
    pub sender_leaf: u32,
    pub plaintext: Vec<u8>,
}

/// One member's live view of one MLS group. Persist via `to_bytes`,
/// reload via `from_bytes` — the bytes hold this member's signing key and
/// every private tree node they're entitled to, so store them exactly as
/// securely as a 1:1 ratchet session.
#[derive(uniffi::Object)]
pub struct FfiMlsGroup(Mutex<GroupState>);

#[uniffi::export]
impl FfiMlsGroup {
    /// Founds a new group with this identity as its sole member.
    #[uniffi::constructor]
    pub fn create(group_id: Vec<u8>, identity_seed: Vec<u8>) -> FfiResult<Arc<Self>> {
        let sk = signing_key(&identity_seed)?;
        let state = GroupState::create(group_id, sk, &mut OsRng);
        Ok(Arc::new(Self(Mutex::new(state))))
    }

    /// Joins from a Welcome — `leaf_secret` is the secret half of the leaf
    /// key whose public half was in the key package the committer added.
    #[uniffi::constructor]
    pub fn join(welcome_bytes: Vec<u8>, identity_seed: Vec<u8>, leaf_secret: Vec<u8>) -> FfiResult<Arc<Self>> {
        let sk = signing_key(&identity_seed)?;
        let ls = StaticSecret::from(seed32(&leaf_secret, "leaf secret")?);
        let welcome = Welcome::from_bytes(&welcome_bytes).map_err(|_| mls_err("malformed welcome"))?;
        let state = GroupState::join(&welcome, sk, ls).map_err(|e| mls_err(&format!("join failed: {e:?}")))?;
        Ok(Arc::new(Self(Mutex::new(state))))
    }

    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        let state = GroupState::from_bytes(&bytes).map_err(|_| mls_err("malformed group state"))?;
        Ok(Arc::new(Self(Mutex::new(state))))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("mls group mutex poisoned").to_bytes()
    }

    pub fn epoch(&self) -> u64 {
        self.0.lock().expect("mls group mutex poisoned").epoch()
    }

    pub fn own_leaf(&self) -> u32 {
        self.0.lock().expect("mls group mutex poisoned").own_leaf()
    }

    pub fn group_id(&self) -> Vec<u8> {
        self.0.lock().expect("mls group mutex poisoned").group_id_bytes().to_vec()
    }

    /// The identity at each leaf — 32 bytes per occupied leaf, empty for a
    /// blank one. Index is the leaf; this is how the app maps a decrypted
    /// message's `sender_leaf` back to a person.
    pub fn roster(&self) -> Vec<Vec<u8>> {
        self.0
            .lock()
            .expect("mls group mutex poisoned")
            .roster()
            .iter()
            .map(|m| m.map(|id| id.0.to_vec()).unwrap_or_default())
            .collect()
    }

    /// Commits `add_key_packages` and `remove_leaves` (either may be
    /// empty; both empty is a self-update for post-compromise security),
    /// applying the result to this group immediately. Use this when there
    /// is no concurrent committer to arbitrate against; otherwise
    /// `build_commit` + `mls_commit_id`.
    pub fn commit(&self, add_key_packages: Vec<Vec<u8>>, remove_leaves: Vec<u32>) -> FfiResult<FfiMlsCommitOutput> {
        let proposals = Self::proposals(add_key_packages, remove_leaves)?;
        let mut state = self.0.lock().expect("mls group mutex poisoned");
        let output = state.commit(proposals, &mut OsRng).map_err(|e| mls_err(&format!("commit failed: {e:?}")))?;
        Ok(Self::encode_output(output, state.to_bytes()))
    }

    /// Builds a commit **without applying it**, returning the messages and
    /// the state to adopt if it wins arbitration. Leaves this group
    /// unchanged, so a member never advances on a commit that loses its
    /// epoch race.
    pub fn build_commit(&self, add_key_packages: Vec<Vec<u8>>, remove_leaves: Vec<u32>) -> FfiResult<FfiMlsCommitOutput> {
        let proposals = Self::proposals(add_key_packages, remove_leaves)?;
        let state = self.0.lock().expect("mls group mutex poisoned");
        let (output, next) = state.build_commit(proposals, &mut OsRng).map_err(|e| mls_err(&format!("build_commit failed: {e:?}")))?;
        Ok(Self::encode_output(output, next.to_bytes()))
    }

    /// Applies another member's commit, advancing this group's epoch.
    pub fn process_commit(&self, commit_bytes: Vec<u8>) -> FfiResult<()> {
        let commit = Commit::from_bytes(&commit_bytes).map_err(|_| mls_err("malformed commit"))?;
        let mut state = self.0.lock().expect("mls group mutex poisoned");
        state.process_commit(&commit).map_err(|e| mls_err(&format!("process_commit failed: {e:?}")))?;
        Ok(())
    }

    /// Replaces this group's state wholesale — used to adopt a
    /// `build_commit` result that won arbitration.
    pub fn adopt_state(&self, state_bytes: Vec<u8>) -> FfiResult<()> {
        let next = GroupState::from_bytes(&state_bytes).map_err(|_| mls_err("malformed group state"))?;
        *self.0.lock().expect("mls group mutex poisoned") = next;
        Ok(())
    }

    pub fn encrypt_message(&self, plaintext: Vec<u8>) -> Vec<u8> {
        self.0.lock().expect("mls group mutex poisoned").encrypt_message(&plaintext)
    }

    pub fn decrypt_message(&self, wire: Vec<u8>) -> FfiResult<FfiMlsMessage> {
        let (sender_leaf, plaintext) = self
            .0
            .lock()
            .expect("mls group mutex poisoned")
            .decrypt_message(&wire)
            .map_err(|e| mls_err(&format!("message rejected: {e:?}")))?;
        Ok(FfiMlsMessage { sender_leaf, plaintext })
    }
}

impl FfiMlsGroup {
    fn proposals(add_key_packages: Vec<Vec<u8>>, remove_leaves: Vec<u32>) -> FfiResult<Vec<Proposal>> {
        let mut proposals = Vec::with_capacity(add_key_packages.len() + remove_leaves.len());
        for leaf in remove_leaves {
            proposals.push(Proposal::Remove(leaf));
        }
        for kp_bytes in add_key_packages {
            let kp: KeyPackage = bincode::deserialize(&kp_bytes).map_err(|_| mls_err("malformed key package"))?;
            proposals.push(Proposal::Add(kp));
        }
        Ok(proposals)
    }

    fn encode_output(output: spiritchat_mls_core::group::CommitOutput, next_state: Vec<u8>) -> FfiMlsCommitOutput {
        FfiMlsCommitOutput {
            commit_bytes: output.commit.to_bytes(),
            welcomes: output
                .welcomes
                .into_iter()
                .map(|(id, welcome)| FfiMlsWelcome { member_identity: id.0.to_vec(), welcome_bytes: welcome.to_bytes() })
                .collect(),
            next_state,
        }
    }
}
