//! Proves the MLS session wrapper end-to-end without any relay involved:
//! group creation, Welcome-based join, application messages both directions,
//! and a self_update commit correctly advancing the epoch for both members.
//! The 3-client-over-a-real-relay test (cli/tests) builds on top of this
//! being correct — if this fails, the bug is in core/, not in the wire.

use chatcore::{ChatSession, Incoming};

#[test]
fn two_party_group_create_join_and_message_both_ways() {
    let mut alice = ChatSession::new("alice");
    let mut bob = ChatSession::new("bob");

    alice.create_group().unwrap();
    assert_eq!(alice.epoch().unwrap(), 0);

    let bob_kp_bundle = bob.generate_key_package().unwrap();
    let welcome_wire = alice.add_members(&[bob_kp_bundle.key_package().clone()]).unwrap();
    let tree_wire = alice.export_ratchet_tree().unwrap();
    assert_eq!(alice.epoch().unwrap(), 1, "founder's commit must advance its own epoch");

    bob.join_from_welcome(&welcome_wire, &tree_wire).unwrap();
    assert_eq!(bob.epoch().unwrap(), 1, "joiner lands at the epoch the Welcome was issued for");

    let msg = alice.encrypt_application(b"hello bob").unwrap();
    match bob.process_incoming(&msg).unwrap() {
        Incoming::Application(bytes) => assert_eq!(bytes, b"hello bob"),
        Incoming::CommitApplied => panic!("expected an application message"),
    }

    let reply = bob.encrypt_application(b"hello alice").unwrap();
    match alice.process_incoming(&reply).unwrap() {
        Incoming::Application(bytes) => assert_eq!(bytes, b"hello alice"),
        Incoming::CommitApplied => panic!("expected an application message"),
    }
}

#[test]
fn self_update_commit_advances_epoch_for_both_members_when_merged() {
    let mut alice = ChatSession::new("alice");
    let mut bob = ChatSession::new("bob");
    alice.create_group().unwrap();
    let bob_kp_bundle = bob.generate_key_package().unwrap();
    let welcome_wire = alice.add_members(&[bob_kp_bundle.key_package().clone()]).unwrap();
    let tree_wire = alice.export_ratchet_tree().unwrap();
    bob.join_from_welcome(&welcome_wire, &tree_wire).unwrap();
    assert_eq!(alice.epoch().unwrap(), bob.epoch().unwrap());
    let starting_epoch = alice.epoch().unwrap();

    let commit_wire = bob.self_update().unwrap();
    bob.merge_pending_commit().unwrap();
    assert_eq!(bob.epoch().unwrap(), starting_epoch + 1);

    match alice.process_incoming(&commit_wire).unwrap() {
        Incoming::CommitApplied => {}
        Incoming::Application(_) => panic!("expected a commit"),
    }
    assert_eq!(
        alice.epoch().unwrap(),
        bob.epoch().unwrap(),
        "both members must converge to the same epoch after the commit is processed"
    );
}

#[test]
fn discarded_pending_commit_leaves_epoch_unchanged() {
    let mut alice = ChatSession::new("alice");
    let mut bob = ChatSession::new("bob");
    alice.create_group().unwrap();
    let bob_kp_bundle = bob.generate_key_package().unwrap();
    let welcome_wire = alice.add_members(&[bob_kp_bundle.key_package().clone()]).unwrap();
    let tree_wire = alice.export_ratchet_tree().unwrap();
    bob.join_from_welcome(&welcome_wire, &tree_wire).unwrap();
    let epoch_before = bob.epoch().unwrap();

    let _losing_commit = bob.self_update().unwrap();
    bob.discard_pending_commit().unwrap();
    assert_eq!(bob.epoch().unwrap(), epoch_before, "a discarded commit must not advance the local epoch");
}
