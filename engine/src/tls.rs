//! Client-side TLS with relay-certificate pinning (T2 / OV2). The client does
//! not trust any CA; it accepts the relay iff the presented leaf certificate
//! hashes to the fingerprint carried in the contact link. This authenticates
//! the relay's transport identity with zero PKI, matching the "the link is the
//! trust anchor" model.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring as provider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};

/// Verifier that accepts exactly one relay certificate, identified by its
/// SHA-256 fingerprint. Chain, expiry, and hostname are all irrelevant — the
/// fingerprint pin is the whole check. The handshake signature is still
/// verified (via the crypto provider) so pinning the cert actually proves the
/// peer holds its private key.
#[derive(Debug)]
struct PinnedFingerprint {
    fingerprint: [u8; 32],
    supported: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedFingerprint {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let got = Sha256::digest(end_entity.as_ref());
        if got.as_slice() == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General(
                "relay TLS certificate does not match the fingerprint pinned in the contact link"
                    .into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }
}

/// A rustls client config that pins `fingerprint`. Uses an explicit ring
/// provider because default features are disabled (no process-global default).
pub fn pinned_client_config(fingerprint: [u8; 32]) -> Arc<ClientConfig> {
    let verifier = PinnedFingerprint {
        fingerprint,
        supported: provider::default_provider().signature_verification_algorithms,
    };
    let config = ClientConfig::builder_with_provider(Arc::new(provider::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Arc::new(config)
}

/// The SNI name sent in the handshake. Our verifier ignores it, but rustls
/// requires a syntactically valid name, so this is a fixed placeholder.
pub fn relay_server_name() -> ServerName<'static> {
    ServerName::try_from("chat-relay").expect("static name is a valid DNS name")
}
