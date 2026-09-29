//! # Storage Layout Snapshot Tests — feat/45
//!
//! The soroban-sdk testutils infrastructure **automatically** writes a JSON
//! snapshot of the full ledger state (storage + events) at the end of every
//! `#[test]` that uses `Env::default()`.  Files land in:
//!
//! ```text
//! contracts/stream/test_snapshots/test/<test_name>.1.json
//! ```
//!
//! Committing those files means any change to the binary XDR encoding of a
//! `#[contracttype]` (added field, renamed variant, type change) produces a
//! visible diff in the PR, and CI fails on unexpected divergence.
//!
//! ## Regenerating snapshots after an intentional schema change
//!
//! ```text
//! UPDATE_EXPECT=true cargo test --package sorostream-stream \
//!     -- storage_layout_snapshot_tests --nocapture
//! ```
//!
//! See CONTRIBUTING.md §"Storage Layout Snapshots" for the full workflow.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ─── Setup ───────────────────────────────────────────────────────────────────

struct SnapEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

fn setup() -> SnapEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    // initialize writes storage_version = 1 (feat/50).
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.add_token_to_whitelist(&admin, &token_id);
    c.set_min_duration(&admin, &0u64);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    SnapEnv { env, contract_id, token_id, sender, recipient, admin }
}

fn client(s: &SnapEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&s.env, &s.contract_id)
}

fn snap_params(nonce: u64) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds: 0,
        nonce,
        renew_count: None,
        recurrence: None,
        lock_until: 0,
        allow_recipient_termination: false,
        non_transferable: false,
        holdback_amount: 0,
        withdrawal_steps: None,
        min_withdrawal_amount: None,
        sponsor: None,
        requires_recipient_approval: false,
    }
}

// ─── Scenario snapshots ───────────────────────────────────────────────────────
// The SDK writes test_snapshots/test/<fn_name>.1.json automatically.
// Commit those files; CI detects format drift via git diff.

/// Snapshot: persistent storage after `create_stream`.
///
/// Captures the initial binary encoding of `Stream`, `StreamOptions`, all
/// index slot keys, and the instance storage counters.
#[test]
fn storage_snapshot_create() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &100_000, &1_000, &false, &snap_params(42),
    );
    // SDK writes snapshot to test_snapshots/test/storage_snapshot_create.1.json
}

/// Snapshot: persistent storage after a mid-stream `withdraw`.
///
/// The `total_withdrawn` and `last_withdraw_time` fields in `StreamOptions`
/// will change. Any encoding change to those fields appears as a diff.
#[test]
fn storage_snapshot_withdraw() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    let stream_id = c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &100_000, &1_000, &false, &snap_params(43),
    );

    s.env.ledger().set_timestamp(1_500); // 50 % elapsed
    c.withdraw(&stream_id, &s.recipient);
    // SDK writes snapshot to test_snapshots/test/storage_snapshot_withdraw.1.json
}

/// Snapshot: persistent storage after `cancel_stream`.
///
/// The stream record is removed on cancel; the snapshot shows only the
/// remaining index/counter entries. A change to the cleanup path will appear.
#[test]
fn storage_snapshot_cancel() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    let stream_id = c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &100_000, &1_000, &false, &snap_params(44),
    );

    s.env.ledger().set_timestamp(1_300); // 30 % elapsed
    c.cancel_stream(&stream_id, &s.sender);
    // SDK writes snapshot to test_snapshots/test/storage_snapshot_cancel.1.json
}

/// Snapshot: persistent storage after `top_up`.
///
/// The `deposit` and `end_time` fields are extended. Any encoding change to
/// the `Stream` struct fields involved in top-up appears as a diff.
#[test]
fn storage_snapshot_top_up() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    let stream_id = c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &100_000, &1_000, &false, &snap_params(45),
    );

    c.top_up(&stream_id, &s.sender, &s.token_id, &50_000);
    // SDK writes snapshot to test_snapshots/test/storage_snapshot_top_up.1.json
}

// ─── feat/50: storage_version ─────────────────────────────────────────────────

/// `initialize` must write `storage_version = 1`.
#[test]
fn test_storage_version_is_one_after_initialize() {
    let s = setup();
    assert_eq!(
        client(&s).get_storage_version(),
        1,
        "storage_version must be 1 after initialize"
    );
}

/// `upgrade_storage` must fail with `MigrationAlreadyApplied` when the
/// contract is already at the current version.
#[test]
fn test_upgrade_storage_already_current() {
    let s = setup();
    let result = client(&s).try_upgrade_storage(&s.admin);
    assert_eq!(
        result,
        Err(Ok(StreamError::MigrationAlreadyApplied)),
        "upgrade_storage must return MigrationAlreadyApplied when already current"
    );
}

/// `upgrade_storage` must succeed and bump the version when the stored version
/// is 0 (legacy deployment without feat/50).
#[test]
fn test_upgrade_storage_from_legacy_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let admin = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "0.9.0"));

    // Simulate a legacy deployment: erase the version key.
    env.storage()
        .instance()
        .remove(&soroban_sdk::Symbol::new(&env, "stor_ver"));

    assert_eq!(c.get_storage_version(), 0, "version must read 0 after key removal");

    // upgrade_storage 0 → 1 must now succeed.
    c.upgrade_storage(&admin);

    assert_eq!(c.get_storage_version(), 1, "version must be 1 after upgrade_storage");
}

/// All four guarded entry points must return `StorageVersionMismatch` when
/// the version key is absent (legacy deployment).
#[test]
fn test_version_mismatch_blocks_create_stream() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "0.9.0"));
    c.add_token_to_whitelist(&admin, &token_id);
    c.set_min_duration(&admin, &0u64);

    env.storage()
        .instance()
        .remove(&soroban_sdk::Symbol::new(&env, "stor_ver"));

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    let result = c.try_create_stream(
        &sender, &recipient, &token_id,
        &100_000, &1_000, &false, &snap_params(99),
    );
    assert_eq!(
        result,
        Err(Ok(StreamError::StorageVersionMismatch)),
        "create_stream must return StorageVersionMismatch on legacy deployment"
    );
}

// ─── feat/45: cleanup_expired_stream ─────────────────────────────────────────

/// An Active stream with a non-zero balance must be rejected.
#[test]
fn test_cleanup_rejects_active_stream() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    let stream_id = c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &100_000, &1_000, &false, &snap_params(60),
    );

    s.env.ledger().set_timestamp(1_200); // still active, balance > 0
    let result = c.try_cleanup_expired_stream(&stream_id, &s.sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotExpired)),
        "cleanup_expired_stream must reject an active stream with non-zero balance"
    );
}

/// An already-completed (auto-removed) stream returns `StreamNotFound`.
#[test]
fn test_cleanup_completed_stream_returns_not_found() {
    let s = setup();
    let c = client(&s);
    s.env.ledger().set_timestamp(1_000);

    let stream_id = c.create_stream(
        &s.sender, &s.recipient, &s.token_id,
        &1_000, &1_000, &false, &snap_params(61),
    );

    // Advance past end_time and fully drain — contract auto-removes the record.
    s.env.ledger().set_timestamp(2_001);
    c.withdraw(&stream_id, &s.recipient);

    let result = c.try_cleanup_expired_stream(&stream_id, &s.sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotFound)),
        "stream is auto-removed on full withdrawal; cleanup returns StreamNotFound"
    );
}
