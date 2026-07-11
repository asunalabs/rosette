//! OV4: the epoch-conflict auto-retry loop. Two paired engines commit
//! concurrently against the same epoch; the relay's DS rule lets exactly one
//! win, and the loser's engine must resolve the conflict autonomously —
//! discard its stale commit, apply the winner's, rebuild at the new epoch,
//! resend — with no manual choreography (which is all the pre-engine test
//! harness could do). Both calls must simply return Ok.

use std::sync::Arc;

use engine::{ChatEngine, Event};
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

async fn paired_engines(addr: &str, fp: [u8; 32]) -> (ChatEngine, ChatEngine) {
    let mut listener = ChatEngine::connect("alice", addr, fp).await.unwrap();
    let link = listener.contact_link().unwrap();
    let scanner = ChatEngine::pair_with_link("bob", &link).await.unwrap();
    listener.await_pairing().await.unwrap();
    (listener, scanner)
}

#[tokio::test]
async fn concurrent_commits_auto_retry_and_converge() {
    let (addr, fp) = start_relay().await;
    let (mut alice, mut bob) = paired_engines(&addr, fp).await;
    assert_eq!(alice.epoch().unwrap(), 1);
    assert_eq!(bob.epoch().unwrap(), 1);

    // Both build + send a commit against epoch 1. Exactly one wins the
    // epoch; the other's engine must auto-retry. Neither call may error.
    let (alice_epoch, bob_epoch) =
        tokio::join!(alice.commit_self_update(), bob.commit_self_update());
    let alice_epoch = alice_epoch.expect("winner or auto-retried loser, never an error");
    let bob_epoch = bob_epoch.expect("winner or auto-retried loser, never an error");

    // The winner lands at epoch 2; the loser rebuilt on top of it → 3.
    let mut landed = [alice_epoch, bob_epoch];
    landed.sort();
    assert_eq!(
        landed,
        [2, 3],
        "one commit wins epoch 1→2, the retried one lands 2→3"
    );

    // The loser already applied the winner's commit during its retry drain
    // (and surfaces it as a buffered event); the winner still has to apply
    // the loser's rebuilt commit off the fan-out.
    let (winner, loser) = if alice_epoch == 2 {
        (&mut alice, &mut bob)
    } else {
        (&mut bob, &mut alice)
    };
    assert_eq!(loser.next_event().await.unwrap(), Event::EpochAdvanced(2));
    assert_eq!(winner.next_event().await.unwrap(), Event::EpochAdvanced(3));
    assert_eq!(alice.epoch().unwrap(), 3);
    assert_eq!(bob.epoch().unwrap(), 3);

    // Real convergence, not just matching epoch numbers: both directions
    // still encrypt/decrypt.
    alice.send_message(b"from alice").await.unwrap();
    assert_eq!(
        bob.next_event().await.unwrap(),
        Event::Message(b"from alice".to_vec())
    );
    bob.send_message(b"from bob").await.unwrap();
    assert_eq!(
        alice.next_event().await.unwrap(),
        Event::Message(b"from bob".to_vec())
    );
}
