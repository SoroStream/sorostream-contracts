use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, Vec,
};

struct TestEnv {
    env: Env,
    admin: Address,
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
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        admin,
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
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #401: Automatic Stream Expiry and Cleanup for Ended Streams
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_401_automatic_stream_expiry_view() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &default_params(),
    );

    // Mid-stream check
    t.env.ledger().set_timestamp(500);
    let stream_mid = c.get_stream(&stream_id);
    assert_eq!(stream_mid.status, StreamStatus::Active);

    // Advance past end_time
    t.env.ledger().set_timestamp(1001);
    let stream_ended = c.get_stream(&stream_id);
    assert_eq!(stream_ended.status, StreamStatus::Expired);
}

#[test]
fn test_issue_401_mark_expired_transition() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &1000u64,
        &false,
        &default_params(),
    );

    t.env.ledger().set_timestamp(1005);
    c.mark_expired(&stream_id);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);
}

#[test]
fn test_issue_401_prune_expired_streams_by_admin() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &200_000,
        &500u64,
        &false,
        &default_params(),
    );

    let stream_2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &300_000,
        &5000u64,
        &false,
        &default_params(),
    );

    // Fast-forward to 1000s (stream 1 ended, stream 2 active)
    t.env.ledger().set_timestamp(1000);

    let ids_to_prune = soroban_sdk::vec![&t.env, stream_1, stream_2];
    let pruned = c.prune_expired_streams(&t.admin, &ids_to_prune);
    assert_eq!(pruned, 1);

    // Stream 1 pruned and no longer in storage
    assert!(c.try_get_stream(&stream_1).is_err());

    // Stream 2 should still be in storage
    let active_stream = c.get_stream(&stream_2);
    assert_eq!(active_stream.status, StreamStatus::Active);
}

#[test]
fn test_issue_401_prune_expired_streams_unauthorized() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &200_000,
        &500u64,
        &false,
        &default_params(),
    );

    t.env.ledger().set_timestamp(1000);

    let non_admin = Address::generate(&t.env);
    let ids = soroban_sdk::vec![&t.env, stream_id];
    let res = c.try_prune_expired_streams(&non_admin, &ids);
    assert!(res.is_err());
}
