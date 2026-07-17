//! MLS session wrapper. All chats are MLS groups — a 1:1 chat is a
//! 2-member group (design doc architecture sketch). This is the only place
//! in the workspace that touches OpenMLS directly; proto/, relay/, and cli/
//! only ever see opaque wire bytes.
//!
//! Persistence (A8/A9): the session runs on `openmls_memory_storage` and is
//! made durable by `snapshot`/`restore` — the whole storage map plus the
//! identity, serialized as one blob the engine writes into the SQLCipher
//! `Store` (storage.rs) after every state-changing operation, atomically
//! with the seen-set and before any ack. Encryption-at-rest is SQLCipher's,
//! not the snapshot's — the blob itself is plaintext to whoever holds the
//! database key.
// ponytail: whole-map snapshot per operation, not a granular openmls
// StorageProvider impl — a 2-member group's map is tiny. Implement the real
// trait against rusqlite if profiling ever shows snapshot cost at group scale.

use openmls::prelude::tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};
use openmls::prelude::*;
use sha2::{Digest, Sha256};

use crate::identity::Identity;
use crate::provider::{Provider, CIPHERSUITE};

/// A wire message crossed some MLS boundary and produced this locally
/// significant outcome.
pub enum Incoming {
    /// A plaintext application message, ready to show the user.
    Application(Vec<u8>),
    /// A commit was processed and merged. The group has advanced to a new
    /// epoch; no payload to show, but callers may want to notify the user
    /// of a membership or key update.
    CommitApplied,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("no active group — call create_group or join_from_welcome first")]
    NoGroup,
    #[error("MLS wire decode failed: {0}")]
    Decode(String),
    #[error("expected a Welcome message, got a different MLS message type")]
    NotAWelcome,
    #[error("expected a handshake/application message, got a different MLS message type")]
    NotAProtocolMessage,
    #[error("openmls error: {0}")]
    Mls(String),
}

/// Dedup key for at-least-once delivery (amendment A3): a pure function of
/// the wire bytes, so sender and every receiver compute the identical id
/// without any session-side bookkeeping.
pub fn message_id_for(wire_bytes: &[u8]) -> [u8; 16] {
    let digest = Sha256::digest(wire_bytes);
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

pub struct ChatSession {
    provider: Provider,
    identity: Identity,
    group: Option<MlsGroup>,
}

/// Everything `restore` needs to rebuild a live session: the identity
/// (signer + credential), the group id to re-load the `MlsGroup` handle by,
/// and the full openmls storage map (which holds the actual group/ratchet
/// state — `MlsGroup` is just a handle over it).
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionSnapshot {
    credential_with_key: CredentialWithKey,
    signer: openmls_basic_credential::SignatureKeyPair,
    group_id: Option<Vec<u8>>,
    storage: std::collections::HashMap<Vec<u8>, Vec<u8>>,
}

/// Identity-only copy of a `snapshot()` blob for the recovery backup
/// (issue #2): group handle and MLS storage stripped, because ratchet state
/// must never time-travel through a backup. `restore` accepts the result as
/// a fresh, unpaired session with the same identity.
pub fn strip_snapshot_to_identity(bytes: &[u8]) -> Result<Vec<u8>, SessionError> {
    let mut snap: SessionSnapshot =
        bincode::deserialize(bytes).map_err(|e| SessionError::Decode(e.to_string()))?;
    snap.group_id = None;
    snap.storage.clear();
    bincode::serialize(&snap).map_err(|e| SessionError::Decode(e.to_string()))
}

impl ChatSession {
    /// Exposed so `pairing::key_package_from_link` can validate a scanned
    /// KeyPackage against this identity's own crypto backend.
    pub fn provider(&self) -> &Provider {
        &self.provider
    }

    pub fn new(display_name: &str) -> Self {
        let provider = Provider::default();
        let identity = Identity::generate(display_name, &provider);
        ChatSession {
            provider,
            identity,
            group: None,
        }
    }

    /// Serialize the entire session — identity, group handle, and the full
    /// openmls storage map — into one blob for the encrypted store.
    pub fn snapshot(&self) -> Result<Vec<u8>, SessionError> {
        let storage = self
            .provider
            .storage()
            .values
            .read()
            .expect("storage lock is never poisoned — no panics while held")
            .clone();
        let snap = SessionSnapshot {
            credential_with_key: self.identity.credential_with_key.clone(),
            signer: self.identity.signer.clone(),
            group_id: self
                .group
                .as_ref()
                .map(|g| g.group_id().as_slice().to_vec()),
            storage,
        };
        bincode::serialize(&snap).map_err(|e| SessionError::Decode(e.to_string()))
    }

    /// Rebuild a live session from a `snapshot` blob.
    pub fn restore(bytes: &[u8]) -> Result<Self, SessionError> {
        let snap: SessionSnapshot =
            bincode::deserialize(bytes).map_err(|e| SessionError::Decode(e.to_string()))?;
        let provider = Provider::default();
        *provider
            .storage()
            .values
            .write()
            .expect("storage lock is never poisoned — no panics while held") = snap.storage;
        let group = match snap.group_id {
            Some(gid) => Some(
                MlsGroup::load(provider.storage(), &GroupId::from_slice(&gid))
                    .map_err(|e| SessionError::Mls(e.to_string()))?
                    .ok_or_else(|| {
                        SessionError::Mls("snapshot names a group its storage lacks".into())
                    })?,
            ),
            None => None,
        };
        Ok(ChatSession {
            provider,
            identity: Identity {
                credential_with_key: snap.credential_with_key,
                signer: snap.signer,
            },
            group,
        })
    }

    /// A fresh KeyPackage this identity can be invited with — the thing a
    /// contact link's `key_package` bytes actually are (amendment A4).
    pub fn generate_key_package(&self) -> Result<KeyPackageBundle, SessionError> {
        KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.identity.signer,
                self.identity.credential_with_key.clone(),
            )
            .map_err(|e| SessionError::Mls(e.to_string()))
    }

    /// Founds a brand-new group with only this identity as a member.
    pub fn create_group(&mut self) -> Result<(), SessionError> {
        let group = MlsGroup::new(
            &self.provider,
            &self.identity.signer,
            &MlsGroupCreateConfig::default(),
            self.identity.credential_with_key.clone(),
        )
        .map_err(|e| SessionError::Mls(e.to_string()))?;
        self.group = Some(group);
        Ok(())
    }

    /// Adds members by KeyPackage and merges the resulting commit locally.
    /// Returns (commit_wire, welcome_wire) — the commit is only meaningful
    /// to existing members (irrelevant to a v0.1 skeleton's founding step,
    /// where it never touches the relay — see amendment A1's test design);
    /// the welcome is what travels to each new member's bootstrap queue.
    pub fn add_members(&mut self, key_packages: &[KeyPackage]) -> Result<Vec<u8>, SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        let (_commit, welcome, _group_info) = group
            .add_members(&self.provider, &self.identity.signer, key_packages)
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        welcome
            .tls_serialize_detached()
            .map_err(|e| SessionError::Decode(e.to_string()))
    }

    /// The current group's ratchet tree, serialized for out-of-band transfer
    /// alongside a Welcome (design doc: "The public tree is needed and
    /// transferred out of band").
    pub fn export_ratchet_tree(&self) -> Result<Vec<u8>, SessionError> {
        let group = self.group.as_ref().ok_or(SessionError::NoGroup)?;
        group
            .export_ratchet_tree()
            .tls_serialize_detached()
            .map_err(|e| SessionError::Decode(e.to_string()))
    }

    /// Joins a group from a received Welcome + out-of-band ratchet tree.
    pub fn join_from_welcome(
        &mut self,
        welcome_wire: &[u8],
        ratchet_tree_wire: &[u8],
    ) -> Result<(), SessionError> {
        let mut cursor = welcome_wire;
        let msg_in = MlsMessageIn::tls_deserialize(&mut cursor)
            .map_err(|e| SessionError::Decode(e.to_string()))?;
        let welcome = match msg_in.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => return Err(SessionError::NotAWelcome),
        };
        let mut tree_cursor = ratchet_tree_wire;
        let ratchet_tree = RatchetTreeIn::tls_deserialize(&mut tree_cursor)
            .map_err(|e| SessionError::Decode(e.to_string()))?;

        let staged = StagedWelcome::new_from_welcome(
            &self.provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            Some(ratchet_tree),
        )
        .map_err(|e| SessionError::Mls(e.to_string()))?;
        let group = staged
            .into_group(&self.provider)
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        self.group = Some(group);
        Ok(())
    }

    /// The display names of every current group member, from their
    /// BasicCredential identity bytes. Local decoration only (see
    /// `Identity::generate`) — trust rests on TOFU + safety numbers, never
    /// on these strings.
    pub fn member_names(&self) -> Result<Vec<String>, SessionError> {
        let group = self.group.as_ref().ok_or(SessionError::NoGroup)?;
        Ok(group
            .members()
            .map(|m| String::from_utf8_lossy(m.credential.serialized_content()).into_owned())
            .collect())
    }

    pub fn epoch(&self) -> Result<u64, SessionError> {
        Ok(self
            .group
            .as_ref()
            .ok_or(SessionError::NoGroup)?
            .epoch()
            .as_u64())
    }

    /// Builds a self-update commit (a plain, membership-preserving epoch
    /// advance — what the concurrent-commit-conflict test uses to generate
    /// two racing commits). Does NOT merge — the caller only merges after
    /// the relay confirms this commit won its epoch (amendment A1); on loss,
    /// call `discard_pending_commit` instead.
    pub fn self_update(&mut self) -> Result<Vec<u8>, SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        let bundle = group
            .self_update(
                &self.provider,
                &self.identity.signer,
                LeafNodeParameters::default(),
            )
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        bundle
            .commit()
            .tls_serialize_detached()
            .map_err(|e| SessionError::Decode(e.to_string()))
    }

    pub fn merge_pending_commit(&mut self) -> Result<(), SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(|e| SessionError::Mls(e.to_string()))
    }

    /// Discards a locally built but not-yet-merged commit — what the loser
    /// of a concurrent-commit conflict does before processing the winner
    /// (amendment A1: "the group must converge").
    pub fn discard_pending_commit(&mut self) -> Result<(), SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        group
            .clear_pending_commit(self.provider.storage())
            .map_err(|e| SessionError::Mls(e.to_string()))
    }

    pub fn encrypt_application(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        let msg = group
            .create_message(&self.provider, &self.identity.signer, plaintext)
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        msg.tls_serialize_detached()
            .map_err(|e| SessionError::Decode(e.to_string()))
    }

    /// Processes an incoming commit or application message. Commits are
    /// merged immediately — by construction, any commit a v0.1 client
    /// receives via relay fan-out already won its epoch's DS conflict
    /// check (the relay enforces that before ever forwarding one), so there
    /// is nothing left to arbitrate client-side.
    pub fn process_incoming(&mut self, wire_bytes: &[u8]) -> Result<Incoming, SessionError> {
        let group = self.group.as_mut().ok_or(SessionError::NoGroup)?;
        let mut cursor = wire_bytes;
        let msg_in = MlsMessageIn::tls_deserialize(&mut cursor)
            .map_err(|e| SessionError::Decode(e.to_string()))?;
        let protocol_message: ProtocolMessage = match msg_in.extract() {
            MlsMessageBodyIn::PrivateMessage(m) => m.into(),
            MlsMessageBodyIn::PublicMessage(m) => m.into(),
            _ => return Err(SessionError::NotAProtocolMessage),
        };
        let processed = group
            .process_message(&self.provider, protocol_message)
            .map_err(|e| SessionError::Mls(e.to_string()))?;
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => {
                Ok(Incoming::Application(app.into_bytes()))
            }
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                group
                    .merge_staged_commit(&self.provider, *staged)
                    .map_err(|e| SessionError::Mls(e.to_string()))?;
                Ok(Incoming::CommitApplied)
            }
            _ => Err(SessionError::NotAProtocolMessage),
        }
    }
}
