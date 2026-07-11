//! Contact link (QR/URL) format. Amendment A2: versioned + multi-endpoint from
//! day one, so adding backup queues later (Open Question 11) is additive, not
//! a breaking change to every link already printed or shared.
//!
//! Amendment A4: pairing is MLS-native — the link carries a KeyPackage plus
//! the bootstrap queue endpoint(s); the scanner creates the group and sends a
//! standard RFC 9420 Welcome back through that queue. No X3DH/PQXDH.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const LINK_VERSION_V1: u8 = 1;

/// Opaque 32-byte queue identifier. The relay never learns a stable user
/// identity — only queue IDs (see relay/ auth model).
pub type QueueId = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// "host:port" — deliberately a bare string, not a parsed URL, so unknown
    /// future relay addressing schemes don't require a link format bump.
    pub relay_addr: String,
    /// SHA-256 of the relay's self-signed TLS certificate (T2 / OV2). The
    /// scanner pins this: it connects over TLS and rejects the relay unless the
    /// presented leaf cert hashes to exactly this value. No CA, no trust store —
    /// the link IS the trust anchor for the relay's transport identity.
    pub relay_fingerprint: [u8; 32],
    pub queue_id: QueueId,
    /// Per-queue send key, established at the queue's creation and handed to
    /// the scanner via this link. "No accounts" never means "no send
    /// authorization" (design doc, relay/ sketch) — possession of this key is
    /// what authorizes a write to the bootstrap mailbox.
    pub send_key: [u8; 32],
}

/// A contact link: what one QR code or invite link encodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactLink {
    pub version: u8,
    /// Serialized MLS KeyPackage (opaque to proto/ — core/ owns the OpenMLS
    /// types). Single-use per RFC 9420; see A4's last-resort-KeyPackage note
    /// for persistent "add me" QR codes.
    pub key_package: Vec<u8>,
    /// At least one endpoint. v0.1 uses exactly one; multi-endpoint exists so
    /// a relay-loss mitigation (Open Question 11) never needs a v2 format.
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("unsupported contact link version: {0}")]
    UnsupportedVersion(u8),
    #[error("contact link has no endpoints")]
    NoEndpoints,
    #[error("contact link has an empty key package")]
    EmptyKeyPackage,
    #[error("malformed contact link bytes: {0}")]
    Decode(String),
}

impl ContactLink {
    pub fn new(key_package: Vec<u8>, endpoints: Vec<Endpoint>) -> Result<Self, LinkError> {
        let link = ContactLink {
            version: LINK_VERSION_V1,
            key_package,
            endpoints,
        };
        link.validate()?;
        Ok(link)
    }

    pub fn validate(&self) -> Result<(), LinkError> {
        if self.version != LINK_VERSION_V1 {
            return Err(LinkError::UnsupportedVersion(self.version));
        }
        if self.endpoints.is_empty() {
            return Err(LinkError::NoEndpoints);
        }
        if self.key_package.is_empty() {
            return Err(LinkError::EmptyKeyPackage);
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        crate::encode(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LinkError> {
        let link: ContactLink =
            crate::decode(bytes).map_err(|e| LinkError::Decode(e.to_string()))?;
        link.validate()?;
        Ok(link)
    }

    /// Primary bootstrap endpoint. v0.1 always uses the first entry; a future
    /// client picks among endpoints when Open Question 11 lands.
    pub fn primary_endpoint(&self) -> &Endpoint {
        &self.endpoints[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_endpoint() -> Endpoint {
        Endpoint {
            relay_addr: "relay.example:8443".to_string(),
            relay_fingerprint: [6u8; 32],
            queue_id: [7u8; 32],
            send_key: [8u8; 32],
        }
    }

    #[test]
    fn roundtrip() {
        let link = ContactLink::new(vec![1, 2, 3], vec![sample_endpoint()]).unwrap();
        let bytes = link.to_bytes();
        let decoded = ContactLink::from_bytes(&bytes).unwrap();
        assert_eq!(link, decoded);
    }

    #[test]
    fn multi_endpoint_roundtrip() {
        let mut e2 = sample_endpoint();
        e2.queue_id = [9u8; 32];
        let link = ContactLink::new(vec![1, 2, 3], vec![sample_endpoint(), e2]).unwrap();
        let decoded = ContactLink::from_bytes(&link.to_bytes()).unwrap();
        assert_eq!(decoded.endpoints.len(), 2);
    }

    #[test]
    fn rejects_no_endpoints() {
        assert_eq!(
            ContactLink::new(vec![1], vec![]).unwrap_err(),
            LinkError::NoEndpoints
        );
    }

    #[test]
    fn rejects_empty_key_package() {
        assert_eq!(
            ContactLink::new(vec![], vec![sample_endpoint()]).unwrap_err(),
            LinkError::EmptyKeyPackage
        );
    }

    #[test]
    fn rejects_future_version_on_decode() {
        let mut link = ContactLink::new(vec![1], vec![sample_endpoint()]).unwrap();
        link.version = 99;
        let bytes = crate::encode(&link);
        assert_eq!(
            ContactLink::from_bytes(&bytes).unwrap_err(),
            LinkError::UnsupportedVersion(99)
        );
    }

    #[test]
    fn rejects_garbage_bytes() {
        assert!(ContactLink::from_bytes(&[0xff, 0x00, 0x01]).is_err());
    }
}
