//! Tests for Issue #516 — stream reward points.
//!
//! Points accrue to a stream's sender as value streams out of their streams
//! (`1` point per 10_000 stroops withdrawn).  Redeeming points unlocks a
//! discount on the next stream-creation fee, proportional to the points spent.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
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
    StellarAssetClient::new(&env, &token_id).mint(&sender, &20_000_000);

    let admin = Address::generate(&env);
    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    c.set_min_duration(&admin, &0u64);
    c.add_token_to_whitelist(&admin, &token_id);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn params(nonce: u64) -> crate::types::CreateStreamParams {
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

        priority: None,
        tags: None,
    }
}

fn create_stream(t: &TestEnv, amount: i128, duration: u64, nonce: u64) -> u64 {
    let c = client(t);
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &amount,
        &duration,
        &false,
        &params(nonce),
    )
}

#[test]
fn test_516_points_start_at_zero() {
    let t = setup();
    let c = client(&t);
    let stranger = Address::generate(&t.env);
    assert_eq!(c.get_reward_points(&stranger), 0);
    assert_eq!(c.get_fee_discount(&stranger), 0);
}

/// Withdrawing 50_000 stroops (rate 100/s for 500s) grants 50_000 / 10_000 = 5 points.
#[test]
fn test_516_withdraw_accrues_points_for_sender() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = create_stream(&t, 100_000, 1000, 0);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    assert_eq!(c.get_reward_points(&t.sender), 5);
    assert_eq!(
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient),
        50_000
    );
}

#[test]
fn test_516_points_accumulate_across_withdrawals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = create_stream(&t, 100_000, 1000, 0);

    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_reward_points(&t.sender), 3);

    t.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_reward_points(&t.sender), 6);
}

/// 1_000 points unlock 1 bp of discount.  Streaming the full 10_000_000 deposit
/// over 1000s yields exactly 1_000 points.
#[test]
fn test_516_redeem_points_unlocks_discount() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = create_stream(&t, 10_000_000, 1000, 0);

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_reward_points(&t.sender), 1000);

    let discount = c.redeem_points(&t.sender, &1000i128);
    assert_eq!(discount, 1);
    assert_eq!(c.get_fee_discount(&t.sender), 1);
    assert_eq!(c.get_reward_points(&t.sender), 0);
}

#[test]
fn test_516_redeem_more_than_balance_fails() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = create_stream(&t, 100_000, 1000, 0);
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    assert_eq!(
        c.try_redeem_points(&t.sender, &100i128),
        Err(Ok(StreamError::NotAuthorized))
    );
    // Balance is untouched when redemption fails.
    assert_eq!(c.get_reward_points(&t.sender), 5);
}

#[test]
fn test_516_redeem_below_threshold_fails() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    let stream_id = create_stream(&t, 100_000, 1000, 0);
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // 5 points is below the 1_000-point threshold for a single bp.
    assert_eq!(
        c.try_redeem_points(&t.sender, &5i128),
        Err(Ok(StreamError::ZeroAmount))
    );
    assert_eq!(c.get_reward_points(&t.sender), 5);
}

#[test]
fn test_516_redeem_zero_fails() {
    let t = setup();
    let c = client(&t);
    assert_eq!(
        c.try_redeem_points(&t.sender, &0i128),
        Err(Ok(StreamError::ZeroAmount))
    );
}

/// End-to-end: a redeemed discount is applied to (and cleared by) the next
/// stream creation's fee.
#[test]
fn test_516_discount_applied_to_next_creation_fee() {
    let t = setup();
    let c = client(&t);

    // Earn 1_000 points (no creation fee configured yet) and redeem them for a
    // 1 bp discount.
    t.env.ledger().set_timestamp(0);
    let first = create_stream(&t, 10_000_000, 1000, 0);
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&first, &t.recipient);
    c.redeem_points(&t.sender, &1000i128);
    assert_eq!(c.get_fee_discount(&t.sender), 1);

    // Configure a 1_000-stroop creation fee paid in a separate XLM-like token.
    let xlm_admin = Address::generate(&t.env);
    let xlm_token = t
        .env
        .register_stellar_asset_contract_v2(xlm_admin.clone())
        .address();
    StellarAssetClient::new(&t.env, &xlm_token).mint(&t.sender, &1_000_000);
    let treasury = Address::generate(&t.env);
    c.set_creation_fee(&t.admin, &1000i128, &xlm_token);
    c.set_treasury_address(&treasury);

    // Next creation pays the fee discounted by 1 bp: 1000 * 9999 / 10_000 = 999.
    create_stream(&t, 100_000, 1000, 1);
    assert_eq!(
        TokenClient::new(&t.env, &xlm_token).balance(&treasury),
        999
    );
    // The discount is consumed by the creation.
    assert_eq!(c.get_fee_discount(&t.sender), 0);
}
