//! MLS-native pairing (amendment A4). No X3DH/PQXDH: the QR/link carries a
//! KeyPackage plus a bootstrap queue endpoint; the scanner creates the
//! (2-member) group locally and sends a standard RFC 9420 Welcome back
//! through that queue. This file only does the KeyPackage <-> bytes and
//! ContactLink assembly; the actual network round-trip (create the bootstrap
//! mailbox, deliver the Welcome) is orchestration, done by the caller (cli/).
//!
//! v0.1 scope cut (disclosed — see design doc amendment A4's hardening spec):
//! this does not yet enforce Welcome replay rejection, bootstrap-queue
//! one-time-link expiry, or a last-resort-KeyPackage policy for persistent
//! "add me" QR codes. Tracked as T4 in tasks-eng-review-*.jsonl.

use openmls::prelude::tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};
use openmls::prelude::*;
use proto::{ContactLink, Endpoint, LinkError, QueueId};

use crate::provider::Provider;

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error("KeyPackage wire decode failed: {0}")]
    Decode(String),
    #[error("received bytes are not a KeyPackage")]
    NotAKeyPackage,
    #[error("KeyPackage failed validation: {0}")]
    Invalid(String),
}

pub fn key_package_to_bytes(key_package: &KeyPackage) -> Result<Vec<u8>, PairingError> {
    let msg: MlsMessageOut = key_package.clone().into();
    msg.tls_serialize_detached()
        .map_err(|e| PairingError::Decode(e.to_string()))
}

pub fn key_package_from_bytes(
    bytes: &[u8],
    provider: &Provider,
) -> Result<KeyPackage, PairingError> {
    let mut cursor = bytes;
    let msg_in = MlsMessageIn::tls_deserialize(&mut cursor)
        .map_err(|e| PairingError::Decode(e.to_string()))?;
    let kp_in = match msg_in.extract() {
        MlsMessageBodyIn::KeyPackage(kp) => kp,
        _ => return Err(PairingError::NotAKeyPackage),
    };
    kp_in
        .validate(provider.crypto(), ProtocolVersion::Mls10)
        .map_err(|e| PairingError::Invalid(e.to_string()))
}

/// Build the link a QR code encodes: this identity's KeyPackage plus the
/// bootstrap mailbox the relay just minted. `relay_fingerprint` is the relay's
/// TLS cert fingerprint the scanner will pin (T2).
pub fn build_contact_link(
    key_package: &KeyPackage,
    relay_addr: &str,
    relay_fingerprint: [u8; 32],
    queue_id: QueueId,
    send_key: [u8; 32],
) -> Result<ContactLink, PairingError> {
    let kp_bytes = key_package_to_bytes(key_package)?;
    Ok(ContactLink::new(
        kp_bytes,
        vec![Endpoint {
            relay_addr: relay_addr.to_string(),
            relay_fingerprint,
            queue_id,
            send_key,
        }],
    )?)
}

/// The scanner's side: pull a usable KeyPackage back out of a scanned link.
pub fn key_package_from_link(
    link: &ContactLink,
    provider: &Provider,
) -> Result<KeyPackage, PairingError> {
    key_package_from_bytes(&link.key_package, provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ChatSession;

    #[test]
    fn key_package_survives_link_roundtrip() {
        let alice = ChatSession::new("alice");
        let bundle = alice.generate_key_package().unwrap();

        let bob_provider = Provider::default();
        let link = build_contact_link(
            bundle.key_package(),
            "relay.local:7443",
            [9u8; 32],
            [1u8; 32],
            [2u8; 32],
        )
        .unwrap();

        let recovered = key_package_from_link(&link, &bob_provider).unwrap();
        assert_eq!(
            recovered.leaf_node().credential().serialized_content(),
            bundle
                .key_package()
                .leaf_node()
                .credential()
                .serialized_content()
        );
    }
}
