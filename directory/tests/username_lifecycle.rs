//! T15: deleted account's (nickname, discriminator) is permanently
//! reserved, not released for reclaiming — decided in `DirectoryStore`'s
//! module docs (simplest safe choice: a freed handle being reclaimed by a
//! different person is an impersonation/confusion risk; revisit only if
//! handle scarcity becomes a real problem).

use directory::DirectoryStore;
use sqlx::PgPool;

#[sqlx::test]
async fn deleted_accounts_handle_cannot_be_reclaimed_by_a_new_signup(pool: PgPool) {
    let store = DirectoryStore::from_pool(pool);

    let original = store
        .find_or_create_pending_user("original-owner-hash")
        .await
        .unwrap();
    let (slot, width) = store
        .claim_username(original, "reserved_name")
        .await
        .unwrap();
    assert_eq!((slot, width), (1, 2));

    store.erase_user(original).await.unwrap();

    // A different account trying to claim the exact same nickname must
    // land on a *different* discriminator, not reclaim slot 1.
    let newcomer = store
        .find_or_create_pending_user("newcomer-hash")
        .await
        .unwrap();
    let (new_slot, _new_width) = store
        .claim_username(newcomer, "reserved_name")
        .await
        .unwrap();
    assert_ne!(
        new_slot, slot,
        "a deleted account's exact (nickname, discriminator) must stay reserved"
    );
    assert_eq!(
        new_slot, 2,
        "newcomer gets the next free slot, not the freed one"
    );
}
