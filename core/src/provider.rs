//! Crypto backend and ciphersuite choice. One constant, one type alias — the
//! rest of core/ never names a concrete backend, so swapping crypto
//! providers later (e.g. for a hardened backend) touches only this file.

use openmls::prelude::Ciphersuite;

pub type Provider = openmls_rust_crypto::OpenMlsRustCrypto;

pub const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
