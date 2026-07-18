//! OV5: foreign-duplicate suppression. At-least-once delivery (T4) makes
//! duplicates normal, and `ChatSession::process_incoming` hard-errors on MLS
//! replay — so the engine's seen-set is what stands between a redelivered
//! envelope and a crashed client. The peer here is a LOW-LEVEL harness
//! (ChatSession + RelayClient), because injecting a byte-identical duplicate
//! requires driving the wire directly; the engine under test only sees
//! ordinary pushes.

use std::sync::Arc;

use chatcore::{message_id_for, ChatSession};
use engine::pairing::BootstrapPayload;
use engine::{ChatEngine, Event, RelayClient};
use proto::{DeliveryMode, Envelope, GroupSendKind};
use relay::{RelayIdentity, RelayState};

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
    let padded = proto::pad(&wire_bytes).expect("test messages fit the largest bucket");
    Envelope::new(
        message_id_for(&wire_bytes),
        DeliveryMode::RelayFanout,
        padded,
    )
}

#[tokio::test]
async fn duplicate_delivery_is_surfaced_once_and_never_errors() {
    let (addr, fp) = start_relay().await;

    // Low-level peer: session + raw relay client + a mailbox it hands out
    // in a contact link.
    let mut peer_session = ChatSession::new("bob");
    let mut peer_relay = RelayClient::connect(&addr, fp).await.unwrap();
    let (peer_mailbox, peer_mailbox_key) = peer_relay.create_mailbox(None).await.unwrap();
    peer_relay.subscribe(vec![peer_mailbox]).await.unwrap();
    let kp = peer_session.generate_key_package().unwrap();
    let link = chatcore::pairing::build_contact_link(
        kp.key_package(),
        &addr,
        fp,
        peer_mailbox,
        peer_mailbox_key,
    )
    .unwrap();
    let link_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, link.to_bytes());

    // Engine under test pairs by scanning the harness's link…
    let mut alice = ChatEngine::pair_with_link("alice", &link_b64)
        .await
        .unwrap();

    // …and the harness completes its side of the bootstrap by hand.
    let (_qid, bootstrap) = peer_relay.push_rx.recv().await.unwrap();
    let payload: BootstrapPayload = bincode::deserialize(&bootstrap.padded_ciphertext).unwrap();
    peer_session
        .join_from_welcome(&payload.welcome_wire, &payload.tree_wire)
        .unwrap();
    peer_relay.ack(_qid, bootstrap.message_id).await.unwrap();

    // One MLS application message… delivered twice. The second send is a
    // byte-identical envelope — exactly what an at-least-once redelivery
    // looks like on the wire.
    let wire = peer_session.encrypt_application(b"hello").unwrap();
    let envelope = wrap(wire);
    for _ in 0..2 {
        peer_relay
            .send_to_group_inbox(
                payload.inbox_qid,
                &payload.inbox_key,
                GroupSendKind::Application,
                envelope.clone(),
            )
            .await
            .unwrap()
            .unwrap();
    }

    // Surfaced exactly once…
    assert_eq!(
        alice.next_event().await.unwrap(),
        Event::Message(b"hello".to_vec())
    );

    // …and the engine survived the replay: the NEXT thing it surfaces is the
    // next real message, not an MLS replay error and not "hello" again.
    let wire2 = peer_session.encrypt_application(b"world").unwrap();
    peer_relay
        .send_to_group_inbox(
            payload.inbox_qid,
            &payload.inbox_key,
            GroupSendKind::Application,
            wrap(wire2),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        alice.next_event().await.unwrap(),
        Event::Message(b"world".to_vec())
    );
}
