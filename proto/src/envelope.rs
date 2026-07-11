//! Message envelope. Amendment A3: delivery is at-least-once with client-side
//! dedup keyed on `MessageId`. Amendment A5: v1 ships exactly one group
//! delivery path (relay fan-out); `DeliveryMode` reserves the field so a
//! future pairwise mode is additive, not a protocol break.

use serde::{Deserialize, Serialize};

/// Dedup key. Derived from the MLS message's own framing (sender + epoch +
/// generation, hashed) by core/ — proto/ just carries it as an opaque id.
pub type MessageId = [u8; 16];

/// Padding buckets applied to ciphertext length before it leaves core/. Sizes
/// chosen so a bucket boundary also enforces the v1 max message size (A7).
pub const PADDING_BUCKETS: [usize; 5] = [1024, 4096, 16384, 32768, 65536];

/// Reject anything padded past the largest bucket outright — see limits::MAX_MESSAGE_SIZE.
pub fn padded_bucket_for(len: usize) -> Option<usize> {
    PADDING_BUCKETS.iter().copied().find(|&b| len <= b)
}

/// Zero-extends `wire_bytes` up to its padding bucket. Safe for any
/// TLS-codec-framed MLS message (commit, welcome, or application ciphertext):
/// those formats are self-delimiting — decoding reads exactly the encoded
/// length and simply leaves trailing zero padding unread — so no separate
/// length-prefix/unpad step is needed on the receiving side.
pub fn pad(wire_bytes: &[u8]) -> Option<Vec<u8>> {
    let bucket = padded_bucket_for(wire_bytes.len())?;
    let mut padded = wire_bytes.to_vec();
    padded.resize(bucket, 0);
    Some(padded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryMode {
    /// v1 default and only implemented mode: relay-side fan-out from a
    /// per-group inbox queue. Disclosed trade-off: the relay can correlate
    /// this group's recipient queues (see mission test in the design doc).
    RelayFanout,
    /// Reserved, not implemented at v1 (amendment A5). Graph-blind pairwise
    /// delivery for small high-risk groups — needs pairwise queues between
    /// all members, which is exactly the N×N setup cost this app avoids by
    /// default. A future client MUST treat an unknown-future variant here as
    /// unsupported rather than silently falling back to RelayFanout.
    Pairwise,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub message_id: MessageId,
    pub delivery_mode: DeliveryMode,
    /// Already padded to a PADDING_BUCKETS size by the sender's core/ before
    /// encryption; proto/ never inspects or re-pads this.
    pub padded_ciphertext: Vec<u8>,
}

impl Envelope {
    pub fn new(
        message_id: MessageId,
        delivery_mode: DeliveryMode,
        padded_ciphertext: Vec<u8>,
    ) -> Self {
        Envelope {
            message_id,
            delivery_mode,
            padded_ciphertext,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_selection() {
        assert_eq!(padded_bucket_for(1), Some(1024));
        assert_eq!(padded_bucket_for(1024), Some(1024));
        assert_eq!(padded_bucket_for(1025), Some(4096));
        assert_eq!(padded_bucket_for(65536), Some(65536));
        assert_eq!(padded_bucket_for(65537), None);
    }

    #[test]
    fn pad_extends_to_bucket_and_rejects_oversize() {
        assert_eq!(pad(&[1, 2, 3]).unwrap().len(), 1024);
        assert_eq!(pad(&[0u8; 1024]).unwrap().len(), 1024);
        assert_eq!(pad(&[0u8; 1025]).unwrap().len(), 4096);
        assert!(pad(&[0u8; 65537]).is_none());
    }

    #[test]
    fn envelope_roundtrip() {
        let env = Envelope::new([1u8; 16], DeliveryMode::RelayFanout, vec![0u8; 1024]);
        let bytes = crate::encode(&env);
        let decoded: Envelope = crate::decode(&bytes).unwrap();
        assert_eq!(env, decoded);
    }
}
