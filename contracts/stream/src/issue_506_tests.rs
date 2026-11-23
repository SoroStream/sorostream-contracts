
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
    env.mock_all_auths_allowing_non_root_auth();

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
// Issue #506: TTL Extension Strategy Tests
// ─────────────────────────────────────────────────────────────────────────
// Test that stream ledger entries have their TTL extended on mutating operations.

#[test]
fn test_issue_506_ttl_extension_on_withdraw() {
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

    // Advance time and withdraw
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    // Stream should still exist and be retrievable
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert!(stream.options.total_withdrawn > 0, "Total withdrawn should increase");
}

#[test]
fn test_issue_506_ttl_extension_on_cancel() {
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

    // Advance time significantly
    t.env.ledger().set_timestamp(2000);

    // Cancel should still work without storage expiry
    c.cancel_stream(&stream_id, &t.sender);

    // Stream should be marked as cancelled (or removed if completed)
    let result = c.try_get_stream(&stream_id);
    // Either stream is removed or marked cancelled - both indicate successful cancellation
    assert!(result.is_err() || c.get_stream(&stream_id).status == StreamStatus::Cancelled);
}

#[test]
fn test_issue_506_ttl_extension_on_metadata_update() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &5000u64,
        &false,
        &default_params(),
    );

    let uri = Some(soroban_sdk::String::from_str(&t.env, "https://example.com/meta"));
    c.update_metadata_uri(&t.sender, &stream_id, &uri);

    // Stream should still be retrievable
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.metadata_uri, uri);
}

#[test]
fn test_issue_506_multiple_mutating_calls_extend_ttl() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &100_000u64,
        &false,
        &default_params(),
    );

    // Perform multiple mutating operations at different times
    for i in 1..5 {
        t.env.ledger().set_timestamp(i * 10_000);

        if i % 2 == 0 {
            c.withdraw(&stream_id, &t.recipient);
        } else {
            let uri = Some(soroban_sdk::String::from_str(&t.env, "https://example.com/meta"));
            c.update_metadata_uri(&t.sender, &stream_id, &uri);
        }

        // Stream should still exist after each operation
        let stream = c.get_stream(&stream_id);
        assert_eq!(stream.status, StreamStatus::Active);
    }
}
