use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn default_params_with_nonce(nonce: u64) -> crate::types::CreateStreamParams {
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

// ─────────────────────────────────────────────────────────────────────────────
// Issue #609: stream_history ring buffer must be atomic on overflow
// ─────────────────────────────────────────────────────────────────────────────

/// Drive a stream through 6 pause/resume cycles (12 transitions after creation)
/// to overflow the 10-entry ring buffer. Verify the buffer stays exactly at 10
/// and always contains a consistent set of entries (no partial writes).
#[test]
fn test_609_ring_buffer_evicts_oldest_and_stays_consistent() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &1_000u64,
        &false,
        &default_params_with_nonce(0),
    );

    // 6 pause/resume cycles → 13 total transitions (1 creation + 12 state changes)
    // The ring buffer caps at 10, evicting oldest on overflow.
    for cycle in 0..6u64 {
        let pause_ts = 1 + cycle * 2;
        t.env.ledger().set_timestamp(pause_ts);
        c.pause_stream(&stream_id, &t.sender);

        t.env.ledger().set_timestamp(pause_ts + 1);
        c.resume_stream(&stream_id, &t.sender);
    }

    let history = c.get_stream_transitions(&stream_id).unwrap();

    // Buffer must be capped at 10
    assert_eq!(
        history.len(),
        10,
        "ring buffer must hold exactly 10 entries after overflow"
    );

    // Every entry must be a valid Paused↔Active alternation (creation entry evicted)
    for entry in history.iter() {
        assert!(
            !entry.is_creation,
            "creation entry must have been evicted from a full buffer"
        );
        let valid = (entry.from_status == StreamStatus::Active
            && entry.to_status == StreamStatus::Paused)
            || (entry.from_status == StreamStatus::Paused
                && entry.to_status == StreamStatus::Active);
        assert!(valid, "every entry must be a Paused<->Active transition");
    }
}

/// Verify that the buffer is consistent immediately after creation (1 entry).
#[test]
fn test_609_ring_buffer_has_creation_entry() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &1_000u64,
        &false,
        &default_params_with_nonce(1),
    );

    let history = c.get_stream_transitions(&stream_id).unwrap();

    assert_eq!(history.len(), 1, "freshly created stream has 1 transition");
    let first = history.get(0).unwrap();
    assert!(first.is_creation, "first transition must be marked as creation");
    assert_eq!(first.to_status, StreamStatus::Active);
}
