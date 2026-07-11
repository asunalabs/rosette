//! The walking skeleton's hardest and most important test (design doc
//! amendment A1 / review decision D4): three CLI clients talking only
//! through a real relay over TCP, exercising the relay's Delivery Service
//! role under a genuine race — two members commit concurrently, exactly one
//! wins, the loser retries, and all three members converge to the same
//! group state, proven by real MLS-encrypted message exchange afterward.
//!
//! No mocks: the relay in this test is the same `relay::net::serve_on` code
//! path the `relay` binary runs, listening on a real (OS-assigned) TCP port.

use std::collections::HashSet;
use std::sync::Arc;

use chatcore::{message_id_for, ChatSession, Incoming};
use cli::RelayClient;
use proto::{DeliveryMode, Envelope, GroupSendKind, QueueId, RejectionCode};
use relay::{RelayIdentity, RelayState};

/// Starts a real TLS relay on an OS-assigned port and returns its address plus
/// the cert fingerprint clients must pin (T2). In-memory identity — file
/// persistence is only for the long-lived binary.
async fn start_relay() -> (String, [u8; 32]) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let state = Arc::new(RelayState::new());
    let identity = RelayIdentity::generate();
    let fingerprint = identity.fingerprint;
    tokio::spawn(async move {
        relay::net::serve_on(listener, state, &identity).await.ok();
    });
    (addr, fingerprint)
}

fn wrap(wire_bytes: Vec<u8>) -> Envelope {
    let padded = proto::pad(&wire_bytes).expect("skeleton test messages fit the largest bucket");
    Envelope::new(
        message_id_for(&wire_bytes),
        DeliveryMode::RelayFanout,
        padded,
    )
}

/// A member's view of the relay: its own mailbox (for fan-out delivery) and
/// the group inbox credentials, once known.
struct Member {
    session: ChatSession,
    relay: RelayClient,
    mailbox_qid: QueueId,
    /// Unused in this test (nothing sends to a member's mailbox directly —
    /// only the relay's own fan-out does), but a real client always holds
    /// its mailbox send credential to hand out at pairing, so it's kept
    /// here rather than dropped.
    #[allow(dead_code)]
    mailbox_key: [u8; 32],
    /// Message ids this member authored itself — skipped when they arrive
    /// back via the member's own mailbox subscription (amendment A3: at
    /// least once delivery means senders receive their own fan-out too).
    authored: HashSet<[u8; 16]>,
}

impl Member {
    async fn connect(name: &str, relay_addr: &str, fingerprint: [u8; 32]) -> Self {
        let relay = RelayClient::connect(relay_addr, fingerprint).await.unwrap();
        let (mailbox_qid, mailbox_key) = relay.create_mailbox().await.unwrap();
        relay.subscribe(vec![mailbox_qid]).await.unwrap();
        Member {
            session: ChatSession::new(name),
            relay,
            mailbox_qid,
            mailbox_key,
            authored: HashSet::new(),
        }
    }

    /// Drains this member's mailbox until it sees a non-self-authored
    /// message, decrypting/merging it through the MLS session. Real network
    /// delivery, so pushes can arrive interleaved with unrelated traffic.
    /// Every delivery is acked AFTER processing (T4: the ack is what ends
    /// the relay's redelivery obligation, so it must not precede the work).
    async fn recv_next_foreign(&mut self) -> Incoming {
        loop {
            let (qid, envelope) = self
                .relay
                .push_rx
                .recv()
                .await
                .expect("relay connection alive");
            let incoming = if self.authored.remove(&envelope.message_id) {
                None // my own fan-out echo — already applied locally
            } else {
                Some(
                    self.session
                        .process_incoming(&envelope.padded_ciphertext)
                        .unwrap(),
                )
            };
            self.relay.ack(qid, envelope.message_id).await.unwrap();
            if let Some(incoming) = incoming {
                return incoming;
            }
        }
    }
}

#[tokio::test]
async fn three_clients_converge_after_a_concurrent_commit_conflict() {
    let (relay_addr, fp) = start_relay().await;

    let mut alice = Member::connect("alice", &relay_addr, fp).await;
    let mut bob = Member::connect("bob", &relay_addr, fp).await;
    let mut carol = Member::connect("carol", &relay_addr, fp).await;

    // --- Founding: Alice creates the group and adds Bob + Carol in one
    // commit (design doc amendment A1's test design: the founding Add never
    // touches the relay — no DS conflict is possible when there is exactly
    // one committer and zero other members yet). ---
    alice.session.create_group().unwrap();
    let bob_kp = bob.session.generate_key_package().unwrap();
    let carol_kp = carol.session.generate_key_package().unwrap();
    let welcome_wire = alice
        .session
        .add_members(&[bob_kp.key_package().clone(), carol_kp.key_package().clone()])
        .unwrap();
    let tree_wire = alice.session.export_ratchet_tree().unwrap();
    bob.session
        .join_from_welcome(&welcome_wire, &tree_wire)
        .unwrap();
    carol
        .session
        .join_from_welcome(&welcome_wire, &tree_wire)
        .unwrap();
    assert_eq!(alice.session.epoch().unwrap(), 1);
    assert_eq!(bob.session.epoch().unwrap(), 1);
    assert_eq!(carol.session.epoch().unwrap(), 1);

    // --- Alice creates the group inbox at the relay and distributes its
    // credentials as a real MLS-encrypted application message — the group's
    // actual first message, not an out-of-band test shortcut. ---
    let (inbox_qid, inbox_key) = alice
        .relay
        .create_group_inbox(
            1,
            vec![alice.mailbox_qid, bob.mailbox_qid, carol.mailbox_qid],
        )
        .await
        .unwrap();
    let creds_plaintext = bincode::serialize(&(inbox_qid, inbox_key)).unwrap();
    let creds_wire = alice.session.encrypt_application(&creds_plaintext).unwrap();
    let creds_envelope = wrap(creds_wire);
    alice.authored.insert(creds_envelope.message_id);
    alice
        .relay
        .send_to_group_inbox(
            inbox_qid,
            &inbox_key,
            GroupSendKind::Application,
            creds_envelope,
        )
        .await
        .unwrap()
        .expect("creds broadcast must succeed");

    for member in [&mut bob, &mut carol] {
        match member.recv_next_foreign().await {
            Incoming::Application(bytes) => assert_eq!(bytes, creds_plaintext),
            Incoming::CommitApplied => panic!("expected the group-inbox creds application message"),
        }
    }

    // --- The property under test: Bob and Carol build a self-update commit
    // each, then fire both at the group inbox genuinely concurrently. ---
    let bob_commit_wire = bob.session.self_update().unwrap();
    let carol_commit_wire = carol.session.self_update().unwrap();
    let bob_envelope = wrap(bob_commit_wire.clone());
    let carol_envelope = wrap(carol_commit_wire.clone());

    let (bob_result, carol_result) = tokio::join!(
        bob.relay.send_to_group_inbox(
            inbox_qid,
            &inbox_key,
            GroupSendKind::Commit { epoch: 1 },
            bob_envelope.clone()
        ),
        carol.relay.send_to_group_inbox(
            inbox_qid,
            &inbox_key,
            GroupSendKind::Commit { epoch: 1 },
            carol_envelope.clone()
        )
    );
    let bob_result = bob_result.unwrap();
    let carol_result = carol_result.unwrap();

    assert_ne!(
        bob_result.is_ok(),
        carol_result.is_ok(),
        "exactly one of the two concurrent commits must win its epoch"
    );

    // Resolve: the winner merges what it already built; the loser discards
    // its stale pending commit and will catch up from the fan-out below.
    let (winner_name, winner_wire) = if bob_result.is_ok() {
        bob.session.merge_pending_commit().unwrap();
        bob.authored.insert(bob_envelope.message_id);
        carol.session.discard_pending_commit().unwrap();
        assert_eq!(carol_result.unwrap_err(), RejectionCode::EpochConflict);
        ("bob", bob_commit_wire)
    } else {
        carol.session.merge_pending_commit().unwrap();
        carol.authored.insert(carol_envelope.message_id);
        bob.session.discard_pending_commit().unwrap();
        assert_eq!(bob_result.unwrap_err(), RejectionCode::EpochConflict);
        ("carol", carol_commit_wire)
    };
    let _ = winner_wire; // kept for the assertion message below only

    // Alice and the loser must both process the winning commit off their
    // mailbox fan-out to converge.
    for member in [&mut alice, &mut bob, &mut carol] {
        if (member.session.epoch().unwrap()) == 2 {
            continue; // this is the winner — already merged above
        }
        match member.recv_next_foreign().await {
            Incoming::CommitApplied => {}
            Incoming::Application(_) => panic!("expected the winning commit ({winner_name})"),
        }
    }

    assert_eq!(alice.session.epoch().unwrap(), 2);
    assert_eq!(bob.session.epoch().unwrap(), 2);
    assert_eq!(carol.session.epoch().unwrap(), 2);
    assert_eq!(
        alice.session.epoch().unwrap(),
        bob.session.epoch().unwrap(),
        "all three members must converge to the identical epoch"
    );
    assert_eq!(bob.session.epoch().unwrap(), carol.session.epoch().unwrap());

    // --- Real convergence, not just matching epoch numbers: every member
    // can now send and every other member can decrypt. ---
    let members: [(&str, &mut Member); 3] = [
        ("alice", &mut alice),
        ("bob", &mut bob),
        ("carol", &mut carol),
    ];
    let senders = members;
    for i in 0..senders.len() {
        let plaintext = format!("hello from {}", senders[i].0).into_bytes();
        let wire = senders[i]
            .1
            .session
            .encrypt_application(&plaintext)
            .unwrap();
        let envelope = wrap(wire);
        senders[i].1.authored.insert(envelope.message_id);
        let (qid, key) = (inbox_qid, inbox_key);
        senders[i]
            .1
            .relay
            .send_to_group_inbox(qid, &key, GroupSendKind::Application, envelope)
            .await
            .unwrap()
            .unwrap();

        for j in 0..senders.len() {
            if i == j {
                continue;
            }
            match senders[j].1.recv_next_foreign().await {
                Incoming::Application(bytes) => assert_eq!(bytes, plaintext),
                Incoming::CommitApplied => panic!("expected an application message"),
            }
        }
    }
}
