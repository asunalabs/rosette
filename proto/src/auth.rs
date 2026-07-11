//! Per-queue send authorization. "No accounts" never means "no send
//! authorization" — every send must carry a valid HMAC tag over the queue id
//! and envelope, keyed by the secret handed out at queue creation. Shared by
//! both the sender (compute) and the relay (verify) so the two never drift.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::envelope::Envelope;
use crate::link::QueueId;
use crate::wire::AuthTag;

type HmacSha256 = Hmac<Sha256>;

pub fn compute_tag(send_key: &[u8; 32], queue_id: &QueueId, envelope: &Envelope) -> AuthTag {
    let mut mac = HmacSha256::new_from_slice(send_key).expect("HMAC accepts any key length");
    mac.update(queue_id);
    mac.update(&crate::encode(envelope));
    let result = mac.finalize().into_bytes();
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&result);
    tag
}

pub fn verify_tag(
    send_key: &[u8; 32],
    queue_id: &QueueId,
    envelope: &Envelope,
    tag: &AuthTag,
) -> bool {
    compute_tag(send_key, queue_id, envelope) == *tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::DeliveryMode;

    #[test]
    fn matching_key_verifies() {
        let key = [1u8; 32];
        let qid = [2u8; 32];
        let env = Envelope::new([3u8; 16], DeliveryMode::RelayFanout, vec![9u8; 8]);
        let tag = compute_tag(&key, &qid, &env);
        assert!(verify_tag(&key, &qid, &env, &tag));
    }

    #[test]
    fn wrong_key_rejected() {
        let qid = [2u8; 32];
        let env = Envelope::new([3u8; 16], DeliveryMode::RelayFanout, vec![9u8; 8]);
        let tag = compute_tag(&[1u8; 32], &qid, &env);
        assert!(!verify_tag(&[9u8; 32], &qid, &env, &tag));
    }
}
