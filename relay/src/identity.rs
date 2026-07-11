//! Relay TLS identity (T2 / eng-review OV2). The relay presents a self-signed
//! certificate; clients pin its SHA-256 fingerprint (carried in the contact
//! link) rather than trusting any CA. This gives transport confidentiality and
//! authenticates the relay without PKI.
//!
//! The identity MUST be stable across restarts — the fingerprint is baked into
//! every contact link already shared, so a fresh key on restart would brick
//! them. `load_or_create` persists the cert+key to one small file. This is the
//! relay's identity only; full queue-state durability is still T9.

use std::io;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};

pub struct RelayIdentity {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    /// SHA-256 of the DER-encoded end-entity certificate. This is the value a
    /// client pins; the client hashes the presented leaf cert and compares.
    pub fingerprint: [u8; 32],
}

impl RelayIdentity {
    /// Fresh in-memory identity. Used by tests; the binary uses
    /// `load_or_create` so the fingerprint survives restarts.
    pub fn generate() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["chat-relay".to_string()])
            .expect("self-signed cert generation never fails for a static SAN");
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.key_pair.serialize_der();
        let fingerprint = fingerprint_of(&cert_der);
        RelayIdentity {
            cert_der,
            key_der,
            fingerprint,
        }
    }

    /// Load the persisted identity from `path`, or generate + persist one if the
    /// file does not exist. File format: cert DER length (u32 LE) + cert DER +
    /// key DER. Deliberately trivial — one file, one identity.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Self::from_file_bytes(&bytes).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "relay identity file is corrupt; delete it to regenerate",
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let identity = Self::generate();
                std::fs::write(path, identity.to_file_bytes())?;
                Ok(identity)
            }
            Err(e) => Err(e),
        }
    }

    fn to_file_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.cert_der.len() + self.key_der.len());
        out.extend_from_slice(&(self.cert_der.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.cert_der);
        out.extend_from_slice(&self.key_der);
        out
    }

    fn from_file_bytes(bytes: &[u8]) -> Option<Self> {
        let len_field = bytes.get(0..4)?;
        let cert_len = u32::from_le_bytes(len_field.try_into().ok()?) as usize;
        let cert_der = bytes.get(4..4 + cert_len)?.to_vec();
        let key_der = bytes.get(4 + cert_len..)?.to_vec();
        if key_der.is_empty() {
            return None;
        }
        let fingerprint = fingerprint_of(&cert_der);
        Some(RelayIdentity {
            cert_der,
            key_der,
            fingerprint,
        })
    }

    /// The rustls server config presenting this identity. No client auth (send
    /// authorization is the per-queue HMAC, not TLS client certs).
    pub fn server_config(&self) -> Arc<ServerConfig> {
        let certs = vec![CertificateDer::from(self.cert_der.clone())];
        let key = PrivateKeyDer::try_from(self.key_der.clone())
            .expect("stored key DER is a valid PKCS#8 private key");
        // Explicit ring provider: default features are off, so there is no
        // process-global default provider to fall back on.
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("ring provider supports the default protocol versions")
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .expect("cert and key were generated together, so they always match");
        Arc::new(config)
    }

    pub fn fingerprint_hex(&self) -> String {
        self.fingerprint
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

fn fingerprint_of(cert_der: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(cert_der);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_roundtrip_preserves_fingerprint() {
        let id = RelayIdentity::generate();
        let bytes = id.to_file_bytes();
        let loaded = RelayIdentity::from_file_bytes(&bytes).expect("roundtrip decodes");
        assert_eq!(id.fingerprint, loaded.fingerprint);
        assert_eq!(id.cert_der, loaded.cert_der);
        assert_eq!(id.key_der, loaded.key_der);
    }

    #[test]
    fn server_config_builds() {
        // Proves the generated cert+key are accepted by rustls.
        let _ = RelayIdentity::generate().server_config();
    }
}
