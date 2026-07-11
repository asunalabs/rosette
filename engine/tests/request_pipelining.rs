//! T3 (eng-review OV6): the client pipelines — multiple requests in flight
//! on one connection, each reply routed to its caller by `request_id`. The
//! pre-T3 client hard-errored on a second concurrent request ("one
//! outstanding request at a time"), so these tests passing at all proves the
//! new capability; the mixed reply TYPES prove replies land at the right
//! caller (a misrouted reply surfaces as an "expected Ok, got QueueCreated"
//! style failure inside the client helpers).

use std::sync::Arc;

use engine::RelayClient;
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

#[tokio::test]
async fn two_concurrent_in_flight_requests_resolve_correctly() {
    let (addr, fp) = start_relay().await;
    let client = RelayClient::connect(&addr, fp).await.unwrap();

    // Different expected reply types: create_mailbox needs PowChallenge then
    // QueueCreated, subscribe needs Ok. Cross-routing either reply fails the
    // strict type match inside the respective helper.
    let (created, subscribed) = tokio::join!(client.create_mailbox(), client.subscribe(vec![]));
    let (queue_id, _send_key) = created.expect("create_mailbox must succeed while pipelined");
    subscribed.expect("subscribe must succeed while pipelined");
    assert_ne!(queue_id, [0u8; 32]);
}

#[tokio::test]
async fn many_concurrent_mixed_requests_each_get_their_own_reply() {
    let (addr, fp) = start_relay().await;
    let client = RelayClient::connect(&addr, fp).await.unwrap();

    let (a, b, c, d, s1, s2, s3, s4) = tokio::join!(
        client.create_mailbox(),
        client.create_mailbox(),
        client.create_mailbox(),
        client.create_mailbox(),
        client.subscribe(vec![]),
        client.subscribe(vec![]),
        client.subscribe(vec![]),
        client.subscribe(vec![]),
    );
    for subscribed in [s1, s2, s3, s4] {
        subscribed.expect("every pipelined subscribe must resolve with Ok");
    }
    let mut queue_ids = vec![];
    for created in [a, b, c, d] {
        let (queue_id, _key) = created.expect("every pipelined create_mailbox must resolve");
        queue_ids.push(queue_id);
    }
    queue_ids.sort();
    queue_ids.dedup();
    assert_eq!(
        queue_ids.len(),
        4,
        "each caller must receive its own distinct QueueCreated reply"
    );
}
