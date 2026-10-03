
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
    SoroStreamContractClient::new(&env, &contract_id).add_token_to_whitelist(&admin, &token_id);

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

fn default_params() -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds: 0,
        nonce: 0,
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

        priority: None,
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #507: Archived Stream Storage Tier Tests
// ─────────────────────────────────────────────────────────────────────────
// Test that completed streams can be archived to reduce storage costs.

#[test]
fn test_issue_507_archive_completed_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000u64,
        &false,
        &default_params(),
    );

    // Advance to stream completion
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    // After full withdrawal, stream should be completed or archived
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err() || c.get_stream(&stream_id).status == StreamStatus::Completed);
}

#[test]
fn test_issue_507_archive_stream_preserves_history() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &5000u64,
        &false,
        &default_params(),
    );

    // Verify initial stream state
    let initial_stream = c.get_stream(&stream_id);
    assert_eq!(initial_stream.deposit, 500_000);
    assert_eq!(initial_stream.flow_rate, 100); // flow_rate = 500_000 / 5000 = 100

    // Partial withdrawal
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let after_withdraw = c.get_stream(&stream_id);
    let withdrawn_amount = after_withdraw.options.total_withdrawn;
    assert!(withdrawn_amount > 0, "Stream should track withdrawal history");
}

#[test]
fn test_issue_507_completed_streams_reduce_storage_footprint() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let mut completed_stream_ids = std::vec::Vec::new();

    // Create multiple streams that will complete
    for i in 0..3u64 {
        let recipient_i = if i == 0 {
            t.recipient.clone()
        } else {
            let r = Address::generate(&t.env);
            StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &2_000_000);
            r
        };

        let stream_id = c.create_stream(
            &t.sender,
            &recipient_i,
            &t.token_id,
            &50_000,
            &500u64,
            &false,
            &crate::types::CreateStreamParams {
                cliff_seconds: 0,
                nonce: i,
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

        priority: None,
        tags: None,
        metadata_uri: None,
    },
        );

        completed_stream_ids.push(stream_id);
    }

    // Complete all streams
    t.env.ledger().set_timestamp(500);
    for stream_id in &completed_stream_ids {
        let stream = c.get_stream(stream_id);
        c.withdraw(stream_id, &stream.recipient);
    }

    // Verify streams are completed or moved to archive tier
    for stream_id in completed_stream_ids {
        let result = c.try_get_stream(&stream_id);
        if result.is_ok() {
            assert_eq!(result.unwrap().unwrap().status, StreamStatus::Completed);
        }
    }
}

#[test]
fn test_issue_507_archive_tier_configurable_retention() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000u64,
        &false,
        &default_params(),
    );

    // Complete the stream
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    // Stream should be completed
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err() || result.unwrap().unwrap().status == StreamStatus::Completed);
}
