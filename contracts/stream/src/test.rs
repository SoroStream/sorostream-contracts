#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

#[allow(dead_code)]
const WASM: &[u8] =
    include_bytes!("../../../target/wasm32v1-none/release/sorostream_stream.wasm");

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

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

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

#[test]
fn test_create_stream_success() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    assert_eq!(stream_id, 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 100_000);
    assert_eq!(stream.flow_rate, 100);
    assert_eq!(stream.status, StreamStatus::Active);
}

#[test]
fn test_get_all_stream_ids_enumerates_globally() {
    let t = setup();
    let c = client(&t);

    let first_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    let second_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &1u64, &false);
    let third_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &2u64, &false);

    let all_ids = c.get_all_stream_ids(&0u32, &10u32);
    assert_eq!(all_ids.len(), 3);
    assert_eq!(all_ids.get_unchecked(0), first_id);
    assert_eq!(all_ids.get_unchecked(1), second_id);
    assert_eq!(all_ids.get_unchecked(2), third_id);

    let paged_ids = c.get_all_stream_ids(&1u32, &2u32);
    assert_eq!(paged_ids.len(), 2);
    assert_eq!(paged_ids.get_unchecked(0), second_id);
    assert_eq!(paged_ids.get_unchecked(1), third_id);
}

#[test]
fn test_withdraw_partial() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000);
}

#[test]
fn test_withdraw_full() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 100_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Completed);
}

#[test]
fn test_cancel_stream_splits_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    let sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    assert_eq!(recipient_bal, 30_000);
    assert_eq!(sender_bal, 970_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Cancelled);
}

#[test]
fn test_top_up_extends_duration() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    let stream_before = c.get_stream(&stream_id);

    c.top_up(&stream_id, &t.sender, &50_000);

    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.end_time, stream_before.end_time + 500);
    assert_eq!(stream_after.deposit, 150_000);
}

#[test]
fn test_auto_renew_restarts_on_completion() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &200_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &true);

    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);
    assert_eq!(stream.last_withdraw_time, 1000);
}

#[test]
fn test_cannot_withdraw_if_not_recipient() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    let other = Address::generate(&t.env);

    let result = c.try_withdraw(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_cannot_cancel_if_not_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    let other = Address::generate(&t.env);

    let result = c.try_cancel_stream(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_zero_amount_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &0, &1000, &0, &0u64, &false);
    assert!(result.is_err());
}

#[test]
fn test_get_claimable_calculates_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);

    t.env.ledger().set_timestamp(250);
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 25_000);
}

// ── Cliff tests ──────────────────────────────────────────────────────────────

/// Stream: duration=1000s, cliff=500s, flow_rate=100 stroops/s
/// At t=499 (pre-cliff) → claimable must be 0.
#[test]
fn test_cliff_pre_cliff_returns_zero() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // cliff at t=500, end at t=1000
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false);

    t.env.ledger().set_timestamp(499);
    assert_eq!(c.get_claimable(&stream_id), 0);
}

/// At the exact cliff timestamp → claimable reflects time from last_withdraw_time.
/// last_withdraw_time = start = 0, cliff = 500, so elapsed = 500 → 500 * 100 = 50_000.
#[test]
fn test_cliff_at_cliff_returns_accrued() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false);

    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 50_000);
}

/// Post-cliff linear: at t=750, elapsed from start = 750 → 75_000 total accrued.
#[test]
fn test_cliff_post_cliff_linear() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false);

    t.env.ledger().set_timestamp(750);
    assert_eq!(c.get_claimable(&stream_id), 75_000);
}

/// Withdraw while pre-cliff transfers nothing; balance stays 0.
#[test]
fn test_cliff_withdraw_pre_cliff_transfers_nothing() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false);

    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 0);
}

/// cliff_seconds > duration_seconds must fail with InvalidCliff.
#[test]
fn test_cliff_exceeds_duration_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &1001, &0u64, &false);
    assert!(result.is_err());
}

#[test]
fn test_get_admin_returns_initialized_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    assert_eq!(c.get_admin(), admin);
}

#[test]
fn test_set_admin_transfers_role() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    let new_admin = Address::generate(&t.env);
    c.initialize(&admin);
    c.set_admin(&new_admin);
    assert_eq!(c.get_admin(), new_admin);
}

#[test]
fn test_set_admin_rejected_for_non_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    let attacker = Address::generate(&t.env);
    c.initialize(&admin);

    t.env.set_auths(&[]);
    let result = c.try_set_admin(&attacker);
    assert!(result.is_err());
}

#[test]
fn test_admin_persists_across_calls() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    // Interleave unrelated contract calls and re-check admin
    c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    assert_eq!(c.get_admin(), admin);
}

#[test]
fn test_admin_can_pause_and_unpause() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    assert!(!c.is_paused());
    c.pause();
    assert!(c.is_paused());
    c.unpause();
    assert!(!c.is_paused());
}

#[test]
fn test_create_stream_blocked_when_paused() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    c.pause();
    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    assert!(result.is_err());
}

#[test]
fn test_create_stream_works_after_unpause() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    c.pause();
    c.unpause();
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false);
    assert_eq!(stream_id, 0);
}

#[test]
fn test_pause_rejected_for_non_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin);
    t.env.set_auths(&[]);
    assert!(c.try_pause().is_err());
    assert!(c.try_unpause().is_err());
}

/// After passing cliff, tokens accumulate from stream start (not from cliff).
/// cliff=500 in a 1000s stream: at t=500 (cliff) withdraw 50_000, then at t=750 another 25_000.
#[test]
fn test_cliff_accrual_restarts_after_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false);

    // At cliff: 500 * 100 = 50_000 claimable
    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 50_000);
    c.withdraw(&stream_id, &t.recipient);

    // 250 more seconds after withdrawal: 250 * 100 = 25_000
    t.env.ledger().set_timestamp(750);
    assert_eq!(c.get_claimable(&stream_id), 25_000);
}

/// Tokens are not claimable before the cliff, even partway into the stream.
#[test]
fn test_claimable_zero_before_cliff() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // cliff at t=800 within a 1000s stream
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &800, &0u64, &false);

    // at t=500, still before cliff → 0 claimable
    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 0);
}

/// Duration of zero must fail.
#[test]
fn test_zero_duration_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &0, &0, &0u64, &false);
    assert!(result.is_err());
}
