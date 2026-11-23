//! Tests for Issue #468: on-chain per-asset analytics aggregator.
//!
//! `get_stream_analytics(token)` should reflect an incrementally-maintained
//! snapshot (no stream scanning) of total value streamed, streams created,
//! and streams cancelled for that asset.

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

struct TestEnv {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&sender, &0u64);

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    TestEnv {
        env,
        contract,
        token,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract)
}

#[test]
fn analytics_start_at_zero_for_unused_token() {
    let t = setup();
    let c = client(&t);

    let a = c.get_stream_analytics(&t.token);
    assert_eq!(a.token, t.token);
    assert_eq!(a.total_value_streamed, 0);
    assert_eq!(a.total_streams_created, 0);
    assert_eq!(a.total_streams_cancelled, 0);
}

#[test]
fn creation_increments_streams_created() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );
    assert_eq!(c.get_stream_analytics(&t.token).total_streams_created, 1);

    c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &1u64, &false, &0u64, &false,
    );
    assert_eq!(c.get_stream_analytics(&t.token).total_streams_created, 2);

    // Unaffected counters.
    assert_eq!(c.get_stream_analytics(&t.token).total_value_streamed, 0);
    assert_eq!(c.get_stream_analytics(&t.token).total_streams_cancelled, 0);
}

#[test]
fn withdraw_increments_value_streamed_not_created_or_cancelled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    t.env.ledger().set_timestamp(400);
    c.withdraw(&stream_id, &t.recipient);

    let a = c.get_stream_analytics(&t.token);
    assert_eq!(a.total_value_streamed, 400_000);
    assert_eq!(a.total_streams_created, 1);
    assert_eq!(a.total_streams_cancelled, 0);

    // A second withdrawal adds the newly-earned delta, not the cumulative total again.
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_stream_analytics(&t.token).total_value_streamed, 1_000_000);
}

#[test]
fn cancel_increments_cancelled_and_value_streamed() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let a = c.get_stream_analytics(&t.token);
    // Recipient earned 300 seconds * 1000/sec = 300_000 before cancellation.
    assert_eq!(a.total_value_streamed, 300_000);
    assert_eq!(a.total_streams_cancelled, 1);
    assert_eq!(a.total_streams_created, 1);
}

#[test]
fn stop_stream_increments_cancelled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );

    t.env.ledger().set_timestamp(200);
    c.stop_stream(&stream_id, &t.sender);

    let a = c.get_stream_analytics(&t.token);
    assert_eq!(a.total_streams_cancelled, 1);
    assert_eq!(a.total_value_streamed, 200_000);
}

#[test]
fn analytics_are_isolated_per_token() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let token_admin_2 = Address::generate(&t.env);
    let token2 = t
        .env
        .register_stellar_asset_contract_v2(token_admin_2)
        .address();
    StellarAssetClient::new(&t.env, &token2).mint(&t.sender, &10_000_000);

    c.create_stream(
        &t.sender, &t.recipient, &t.token, &1_000_000, &1000, &0,
        &0u64, &false, &0u64, &false,
    );
    c.create_stream(
        &t.sender, &t.recipient, &token2, &1_000_000, &1000, &0,
        &1u64, &false, &0u64, &false,
    );

    assert_eq!(c.get_stream_analytics(&t.token).total_streams_created, 1);
    assert_eq!(c.get_stream_analytics(&token2).total_streams_created, 1);

    // A third, unrelated token that was never used stays at zero.
    let token_admin_3 = Address::generate(&t.env);
    let token3 = t
        .env
        .register_stellar_asset_contract_v2(token_admin_3)
        .address();
    assert_eq!(c.get_stream_analytics(&token3).total_streams_created, 0);
}

#[test]
fn batch_withdraw_and_batch_cancel_update_analytics() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let recipients = soroban_sdk::vec![&t.env, t.recipient.clone(), t.recipient.clone()];
    let amounts = soroban_sdk::vec![&t.env, 1_000_000_i128, 1_000_000_i128];
    let mut tokens = soroban_sdk::Vec::new(&t.env);
    tokens.push_back(t.token.clone());
    tokens.push_back(t.token.clone());
    let lock_untils = soroban_sdk::vec![&t.env, 0u64, 0u64];

    let stream_ids = c.batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false,
        &None::<u32>, &lock_untils, &0u64, &false,
    );
    assert_eq!(c.get_stream_analytics(&t.token).total_streams_created, 2);

    // Each stream has flow_rate = 1_000_000 / 1000 = 1000/sec; at t=500 each
    // has earned 500_000, so batch_withdraw across both moves 1_000_000 total.
    t.env.ledger().set_timestamp(500);
    c.batch_withdraw(&stream_ids, &t.recipient);
    assert_eq!(c.get_stream_analytics(&t.token).total_value_streamed, 1_000_000);

    // Advance further, then cancel both: each has earned another 200_000
    // (t=700) since its last withdrawal, paid out to the recipient at
    // cancellation time.
    t.env.ledger().set_timestamp(700);
    c.batch_cancel_stream(&stream_ids, &t.sender);
    let a = c.get_stream_analytics(&t.token);
    assert_eq!(a.total_streams_cancelled, 2);
    assert_eq!(a.total_value_streamed, 1_000_000 + 400_000);
}
