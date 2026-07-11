//! The step-2 reconnect/resubscribe test: kill the connection mid-session;
//! the engine reconnects, re-subscribes its full queue set, the relay
//! replays the unacked backlog (T4), and the caller observes no loss past
//! the ack point and no duplicates. The kill is real: the engine connects
//! through a TCP proxy whose live connections the test severs, while the
//! relay itself stays up (so the reconnect has somewhere to land).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine::{ChatEngine, Event};
use relay::{RelayIdentity, RelayState};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;

async fn start_relay() -> (String, [u8; 32]) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let state = Arc::new(RelayState::new());
    let identity = RelayIdentity::generate();
    let fingerprint = identity.fingerprint;
    tokio::spawn(async move {
        relay::net::serve_on(listener, state, &identity).await.ok();
    });
    (addr, fingerprint)
}

/// A byte-blind TCP forwarder. TLS passes through untouched, so cert
/// pinning still authenticates the real relay behind it. `kill(n)` aborts
/// the n-th accepted connection (in accept order), closing both sockets —
/// the listener stays alive so reconnects succeed.
struct Proxy {
    addr: String,
    conns: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Proxy {
    async fn start(target: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let conns: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
        let accept_conns = conns.clone();
        tokio::spawn(async move {
            loop {
                let Ok((inbound, _)) = listener.accept().await else {
                    break;
                };
                let target = target.clone();
                let handle = tokio::spawn(async move {
                    let Ok(outbound) = TcpStream::connect(&target).await else {
                        return;
                    };
                    let (mut client_r, mut client_w) = inbound.into_split();
                    let (mut relay_r, mut relay_w) = outbound.into_split();
                    let _ = tokio::join!(
                        tokio::io::copy(&mut client_r, &mut relay_w),
                        tokio::io::copy(&mut relay_r, &mut client_w),
                    );
                });
                accept_conns.lock().unwrap().push(handle);
            }
        });
        Proxy { addr, conns }
    }

    fn kill(&self, index: usize) {
        self.conns.lock().unwrap()[index].abort();
    }
}

async fn expect_message(engine: &mut ChatEngine, expected: &[u8]) {
    let event = timeout(Duration::from_secs(30), engine.next_event())
        .await
        .expect("event must arrive within the timeout")
        .unwrap();
    assert_eq!(event, Event::Message(expected.to_vec()));
}

#[tokio::test]
async fn engine_reconnects_resubscribes_and_replays_backlog_without_loss_or_dup() {
    let (relay_addr, fp) = start_relay().await;
    let proxy = Proxy::start(relay_addr).await;

    // Alice connects THROUGH the proxy (accept #0); her contact link embeds
    // the proxy address, so Bob (accept #1) rides it too — but only
    // connection #0 gets severed.
    let mut alice = ChatEngine::connect("alice", &proxy.addr, fp).await.unwrap();
    let link = alice.contact_link().unwrap();
    let mut bob = ChatEngine::pair_with_link("bob", &link).await.unwrap();
    alice.await_pairing().await.unwrap();

    // A message flows and is acked — this one must NOT come back later.
    bob.send_message(b"one").await.unwrap();
    expect_message(&mut alice, b"one").await;

    // Sever Alice's connection. The relay stays up; Bob stays connected.
    proxy.kill(0);

    // Sent while Alice is down: lands in her mailbox backlog (T4 keeps it
    // until acked).
    bob.send_message(b"two").await.unwrap();

    // Alice's next_event notices the dead connection, reconnects (accept
    // #2), re-subscribes, and the replayed backlog surfaces "two" — not
    // "one" (acked, gone), not an error, not a duplicate.
    expect_message(&mut alice, b"two").await;

    // The fresh connection is fully functional in both directions, and
    // nothing spurious is queued between real messages.
    alice.send_message(b"three").await.unwrap();
    expect_message(&mut bob, b"three").await;
    bob.send_message(b"four").await.unwrap();
    expect_message(&mut alice, b"four").await;
}
