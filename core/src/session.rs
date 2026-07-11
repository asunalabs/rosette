//! MLS session wrapper. All chats are MLS groups — a 1:1 chat is a
//! 2-member group (design doc architecture sketch). This is the only place
//! in the workspace that touches OpenMLS directly; proto/, relay/, and cli/
//! only ever see opaque wire bytes.
//!
//! v0.1 scope cut (disclosed): state lives in memory only
//! (`openmls_memory_storage`), not SQLCipher. Amendments A8/A9 (encrypted
//! export, atomic commit-processing + persistence, crash recovery test) are
//! step-4 work — tracked as T5/T8 in tasks-eng-review-*.jsonl — and require
//! a real storage provider this wrapper doesn't yet have.

use openmls::prelude::tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};
use openmls::prelude::*;
use openmls_memory_storage::MemoryStorage;
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

// MemoryStorage is openmls_memory_storage's concrete provider; named here
// only so the disclosed-cut doc comment above has something to point at in
// rustdoc — Provider (provider.rs) is what's actually used.
#[allow(dead_code)]
type _StorageDoc = MemoryStorage;
