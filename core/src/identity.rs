//! A local identity: an MLS `BasicCredential` plus its signature keypair.
//! No phone number, no server-issued identifier — this is the entire trust
//! root, exactly as much identity as the design doc allows.

use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;

use crate::provider::{Provider, CIPHERSUITE};

pub struct Identity {
    pub credential_with_key: CredentialWithKey,
    pub signer: SignatureKeyPair,
}

impl Identity {
    /// `display_name` is local-only decoration (what the CLI prints), never
    /// transmitted as a stable identifier — MLS BasicCredential identity
    /// bytes are opaque to the relay and to other members' trust decisions,
    /// which rest on TOFU-at-pairing plus safety-number verification, not on
    /// this string.
    pub fn generate(display_name: &str, provider: &Provider) -> Self {
        let credential = BasicCredential::new(display_name.as_bytes().to_vec());
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm())
            .expect("signature scheme is supported by every backend openmls_rust_crypto ships");
        signer
            .store(provider.storage())
            .expect("in-memory storage provider never fails to store");
        Identity {
            credential_with_key: CredentialWithKey {
                credential: credential.into(),
                signature_key: signer.public().into(),
            },
            signer,
        }
    }
}
