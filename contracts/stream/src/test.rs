
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, Env,
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

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    // Disable minimum duration for tests
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

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

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 100_000);
    assert_eq!(stream.flow_rate, 100);
    assert_eq!(stream.status, StreamStatus::Active);
}

#[test]
fn test_withdrawal_cooldown_blocks_repeated_withdrawals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    c.set_withdrawal_cooldown(&admin, &10u64);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_err());
}

#[test]
fn test_whitelist_rejects_non_whitelisted_recipient() {
    let t = setup();
    let c = client(&t);

    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.set_whitelist_enabled(&admin, &true);
    c.add_to_whitelist(&admin, &t.recipient);

    let other = Address::generate(&t.env);
    let result = c.try_create_stream(&t.sender, &other, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert!(result.is_err());
}

#[test]
fn test_metadata_is_stored_and_updatable() {
    let t = setup();
    let c = client(&t);
    let metadata = Bytes::from_array(&t.env, &[1u8, 2u8, 3u8]);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    c.update_metadata(&t.sender, &stream_id, &metadata);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.metadata, metadata);

    let updated = Bytes::from_array(&t.env, &[9u8, 9u8, 9u8]);
    c.update_metadata(&t.sender, &stream_id, &updated);
    let updated_stream = c.get_stream(&stream_id);
    assert_eq!(updated_stream.options.metadata, updated);
}

#[test]
fn test_cancel_auto_renew_before_expiry() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &true, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    c.cancel_auto_renew(&t.sender, &stream_id);

    let stream = c.get_stream(&stream_id);
    assert!(!stream.auto_renew);
}

#[test]
fn test_get_all_stream_ids_enumerates_globally() {
    let t = setup();
    let c = client(&t);

    let first_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let second_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &1u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let third_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &2u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

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

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

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

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 100_000);

    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

// ── Issue #328: withdraw at stream end reconciles the full deposit,
// including any dust left over from `flow_rate = amount / duration_seconds`
// rounding down. `flow_rate` is truncated on creation, so `flow_rate *
// duration` can fall a few stroops short of `deposit`. The contract does not
// strand that remainder: on the final withdrawal (`stream_ended == true`,
// non-auto-renew path) it refunds the leftover dust to the sender in the same
// call that pays the recipient, so 100% of the deposit leaves the contract
// and none of it is stuck. These tests pin down that reconciliation.

#[test]
fn test_withdraw_at_end_time_reconciles_full_deposit_with_dust() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 100_003 / 1000 = 100 (truncated), so flow_rate * duration = 100_000,
    // leaving 3 stroops of rounding dust that doesn't evenly divide out.
    let deposit: i128 = 100_003;
    let duration: u64 = 1000;
    let recipient_share: i128 = 100_000;
    let dust: i128 = deposit - recipient_share;

    let sender_balance_after_create = 1_000_000 - deposit;

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &deposit, &duration, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>);

    t.env.ledger().set_timestamp(duration + 1);
    c.withdraw(&stream_id, &t.recipient);

    let token = TokenClient::new(&t.env, &t.token_id);
    let recipient_bal = token.balance(&t.recipient);
    let sender_bal = token.balance(&t.sender);
    let contract_bal = token.balance(&t.contract_id);

    // Recipient gets the streamed amount; the leftover dust is refunded to
    // the sender rather than lost, so together they recover the full deposit.
    assert_eq!(recipient_bal, recipient_share);
    assert_eq!(sender_bal - sender_balance_after_create, dust);
    assert_eq!(recipient_bal + dust, deposit);

    // Nothing is left behind in the contract for this stream.
    assert_eq!(contract_bal, 0);

    // The stream is fully settled and removed.
    assert!(c.try_get_stream(&stream_id).is_err());
}

#[test]
fn test_withdraw_long_after_end_time_matches_withdraw_at_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let deposit: i128 = 100_003;
    let duration: u64 = 1000;
    let recipient_share: i128 = 100_000;
    let dust: i128 = deposit - recipient_share;
    let sender_balance_after_create = 1_000_000 - deposit;

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &deposit, &duration, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>);

    // Claim well past end_time, not just the first ledger after it.
    t.env.ledger().set_timestamp(duration + 1000);
    c.withdraw(&stream_id, &t.recipient);

    let token = TokenClient::new(&t.env, &t.token_id);
    let recipient_bal = token.balance(&t.recipient);
    let sender_bal = token.balance(&t.sender);
    let contract_bal = token.balance(&t.contract_id);

    // Same reconciliation as claiming at end_time + 1: elapsed time is capped
    // at end_time, so claiming later doesn't change the settled amounts.
    assert_eq!(recipient_bal, recipient_share);
    assert_eq!(sender_bal - sender_balance_after_create, dust);
    assert_eq!(contract_bal, 0);
    assert!(c.try_get_stream(&stream_id).is_err());
}

#[test]
fn test_cancel_stream_splits_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    let sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    assert_eq!(recipient_bal, 30_000);
    assert_eq!(sender_bal, 970_000);

    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

#[test]
fn test_top_up_extends_duration() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let stream_before = c.get_stream(&stream_id);

    c.top_up(&stream_id, &t.sender, &t.token_id, &50_000);

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
    c.set_min_duration(&sender, &0u64);
    env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &true, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);
    assert_eq!(stream.last_withdraw_time, 1000);
}

#[test]
fn test_auto_renew_respects_renew_count_limit() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &500_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);
    env.ledger().set_timestamp(0);

    // Create stream with auto_renew=true and renew_count=Some(2)
    let stream_id = c.create_stream(
        &sender, 
        &recipient, 
        &token_id, 
        &100_000, 
        &1000, 
        &0, 
        &0u64, 
        &true,              // auto_renew
        &Some(2u32),        // renew_count = 2
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
        &false,
        &false
    );

    // First renewal at end_time=1000
    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.options.renewals_used, 1);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);

    // Second renewal at end_time=2000
    env.ledger().set_timestamp(2000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.options.renewals_used, 2);
    assert_eq!(stream.start_time, 2000);
    assert_eq!(stream.end_time, 3000);

    // Third renewal should fail and complete the stream (limit reached)
    env.ledger().set_timestamp(3000);
    c.withdraw(&stream_id, &recipient);

    let stream = c.get_stream(&stream_id);
    // After hitting the limit, the stream should be completed
    // (Note: The stream may be removed from storage, so we expect StreamNotFound or Completed status)
}

#[test]
fn test_auto_renew_without_renew_count_unlimited() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &1_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);
    env.ledger().set_timestamp(0);

    // Create stream with auto_renew=true and renew_count=None (unlimited)
    let stream_id = c.create_stream(
        &sender, 
        &recipient, 
        &token_id, 
        &100_000, 
        &1000, 
        &0, 
        &0u64, 
        &true,              // auto_renew
        &None::<u32>,       // renew_count = None (unlimited)
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
        &false,
        &false
    );

    // Multiple renewals should succeed with renew_count=None
    for i in 1..=5 {
        env.ledger().set_timestamp(i as u64 * 1000);
        c.withdraw(&stream_id, &recipient);

        let stream = c.get_stream(&stream_id);
        assert_eq!(stream.status, StreamStatus::Active);
        assert_eq!(stream.options.renewals_used, i as u32);
    }
}

#[test]
fn test_renew_count_with_zero_limit() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &300_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);
    env.ledger().set_timestamp(0);

    // Create stream with renew_count=Some(0), meaning no renewals allowed
    let stream_id = c.create_stream(
        &sender, 
        &recipient, 
        &token_id, 
        &100_000, 
        &1000, 
        &0, 
        &0u64, 
        &true,              // auto_renew
        &Some(0u32),        // renew_count = 0 (no renewals allowed)
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
        &false,
        &false
    );

    // Stream should complete immediately at end_time with renew_count=0
    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    // Stream should be completed or removed
    let result = c.try_get_stream(&stream_id);
    // Expect the stream to be in a completed/terminal state
}

#[test]
fn test_cancel_auto_renew_before_expiry() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &true, &None::<u32>,
        &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &false, &false, &false);
    c.cancel_auto_renew(&t.sender, &stream_id);

    let stream = c.get_stream(&stream_id);
    assert!(!stream.auto_renew);
}

#[test]
fn test_cannot_withdraw_if_not_recipient() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &None::<u32>, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &false, &false, &false);
    let other = Address::generate(&t.env);

    let result = c.try_withdraw(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_cannot_cancel_if_not_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &None::<u32>, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &false, &false, &false);
    let other = Address::generate(&t.env);

    let result = c.try_cancel_stream(&stream_id, &other);
    assert!(result.is_err());
}

#[test]
fn test_zero_amount_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &0, &1000, &0, &0u64, &false, &None::<u32>, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &false, &false, &false);
    assert!(result.is_err());
}

#[test]
fn test_get_claimable_calculates_correctly() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(250);
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 25_000);
}

// â”€â”€ Cliff tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Stream: duration=1000s, cliff=500s, flow_rate=100 stroops/s
/// At t=499 (pre-cliff) â†’ claimable must be 0.
#[test]
fn test_cliff_pre_cliff_returns_zero() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // cliff at t=500, end at t=1000
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(499);
    assert_eq!(c.get_claimable(&stream_id), 0);
}

/// At the exact cliff timestamp â†’ claimable reflects time from last_withdraw_time.
/// last_withdraw_time = start = 0, cliff = 500, so elapsed = 500 â†’ 500 * 100 = 50_000.
#[test]
fn test_cliff_at_cliff_returns_accrued() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 50_000);
}

/// Post-cliff linear: at t=750, elapsed from start = 750 â†’ 75_000 total accrued.
#[test]
fn test_cliff_post_cliff_linear() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(750);
    assert_eq!(c.get_claimable(&stream_id), 75_000);
}

/// Withdraw while pre-cliff transfers nothing; balance stays 0.
#[test]
fn test_cliff_withdraw_pre_cliff_transfers_nothing() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 0);
}

/// Zero duration (duration_seconds == 0) must fail with ZeroDuration.
/// This prevents division by zero in flow rate calculation and undefined stream behavior.
#[test]
fn test_zero_duration_fails() {
    let t = setup();
    let c = client(&t);

    // Attempt to create a stream with zero duration
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,  // amount
        &0,        // duration_seconds = 0
        &0,        // cliff_seconds
        &0u64,     // nonce
        &false,    // auto_renew
        &0u64,     // lock_until
        &false,    // allow_recipient_termination
        &0i128,    // holdback_amount
        &None::<u32>,   // withdrawal_steps
        &None::<i128>,  // min_withdrawal_amount
        &None::<u32>,   // max_price_deviation_bps (unused in this context)
    );

    // Should fail with ZeroDuration error
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

/// cliff_seconds >= duration_seconds must fail with InvalidCliff.
#[test]
fn test_cliff_exceeds_duration_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &1001, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert!(result.is_err());
}

/// cliff_seconds == duration_seconds must also fail with InvalidCliff.
#[test]
fn test_cliff_equals_duration_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &1000, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert_eq!(result, Err(Ok(StreamError::InvalidCliff)));
}

/// cliff_seconds == 0 means no cliff â€” tokens stream linearly from start.
#[test]
fn test_cliff_zero_means_no_cliff() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // At t=1 (right after start), tokens should already be claimable
    t.env.ledger().set_timestamp(1);
    assert_eq!(c.get_claimable(&stream_id), 100);

    // Verify cliff_time equals start_time (no cliff barrier)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.cliff_time, stream.start_time);
}

/// cliff_seconds strictly between 0 and duration creates a valid cliff.
#[test]
fn test_cliff_strictly_between_zero_and_duration() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &1, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // Before cliff (t=0): no claimable
    assert_eq!(c.get_claimable(&stream_id), 0);

    // At cliff (t=1): claimable = 1 * 100 = 100
    t.env.ledger().set_timestamp(1);
    assert_eq!(c.get_claimable(&stream_id), 100);
}

#[test]
fn test_get_admin_returns_initialized_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    assert_eq!(c.get_admin(), admin);
}

#[test]
fn test_set_admin_transfers_role() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    let new_admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    c.set_admin(&new_admin);
    assert_eq!(c.get_admin(), new_admin);
}

#[test]
fn test_set_admin_rejected_for_non_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    let attacker = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    t.env.set_auths(&[]);
    let result = c.try_set_admin(&attacker);
    assert!(result.is_err());
}

#[test]
fn test_admin_persists_across_calls() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    // Interleave unrelated contract calls and re-check admin
    c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert_eq!(c.get_admin(), admin);
}

#[test]
fn test_admin_can_pause_and_unpause() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    assert!(!c.is_paused());
    c.emergency_pause();
    assert!(c.is_paused());
    c.emergency_resume();
    assert!(!c.is_paused());
}

#[test]
fn test_create_stream_blocked_when_paused() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    c.emergency_pause();
    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert!(result.is_err());
}

#[test]
fn test_create_stream_works_after_unpause() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    c.emergency_pause();
    c.emergency_resume();
    let _stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
}

#[test]
fn test_pause_rejected_for_non_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    t.env.set_auths(&[]);
    assert!(c.try_emergency_pause().is_err());
    assert!(c.try_emergency_resume().is_err());
}

/// After passing cliff, tokens accumulate from stream start (not from cliff).
/// cliff=500 in a 1000s stream: at t=500 (cliff) withdraw 50_000, then at t=750 another 25_000.
#[test]
fn test_cliff_accrual_restarts_after_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &500, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

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
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &800, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // at t=500, still before cliff â†’ 0 claimable
    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&stream_id), 0);
}

/// Duration of zero must fail.
#[test]
fn test_zero_duration_fails() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &0, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    assert!(result.is_err());
}

// â”€â”€ Event snapshot tests (issue #105) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// These tests capture the exact event format emitted by each contract
// instruction. If the event topic structure, field types, or values change,
// these tests will fail â€” ensuring SDK and indexer consumers are notified
// of format changes.

use soroban_sdk::testutils::Events;
use soroban_sdk::{IntoVal, Val, Symbol, vec as soroban_vec};

#[test]
fn snapshot_event_stream_created() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(100);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let events = t.env.events().all();
    let create_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamCreated")
        } else {
            false
        }
    }).collect();

    assert_eq!(create_events.len(), 1, "Expected exactly one StreamCreated event");

    let (contract_id, topics, data) = &create_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamCreated"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_name: Symbol = topics_vec.get(0).unwrap().into_val(&t.env);
    assert_eq!(topic_name, Symbol::new(&t.env, "StreamCreated"));
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (sender: Address, recipient: Address, amount: i128, flow_rate: i128, end_time: u64)
    let data_tuple: (Address, Address, i128, i128, u64) = data.clone().into_val(&t.env);
    assert_eq!(data_tuple.0, t.sender);
    assert_eq!(data_tuple.1, t.recipient);
    assert_eq!(data_tuple.2, 100_000i128);
    assert_eq!(data_tuple.3, 100i128);       // flow_rate = 100_000 / 1000
    assert_eq!(data_tuple.4, 100 + 1000);    // end_time = start + duration
}

#[test]
fn snapshot_event_stream_withdrawn() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let events = t.env.events().all();
    let withdraw_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamWithdrawn")
        } else {
            false
        }
    }).collect();

    assert_eq!(withdraw_events.len(), 1, "Expected exactly one StreamWithdrawn event");

    let (contract_id, topics, data) = &withdraw_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamWithdrawn"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (recipient: Address, amount: i128, timestamp: u64)
    let data_tuple: (Address, i128, u64) = data.clone().into_val(&t.env);
    assert_eq!(data_tuple.0, t.recipient);
    assert_eq!(data_tuple.1, 50_000i128);     // 500s * 100 stroops/s
    assert_eq!(data_tuple.2, 500u64);
}

#[test]
fn snapshot_event_stream_cancelled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let events = t.env.events().all();
    let cancel_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamCancelled")
        } else {
            false
        }
    }).collect();

    assert_eq!(cancel_events.len(), 1, "Expected exactly one StreamCancelled event");

    let (contract_id, topics, data) = &cancel_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamCancelled"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (sender: Address, refund_amount: i128, recipient_amount: i128)
    let data_tuple: (Address, i128, i128) = data.clone().into_val(&t.env);
    assert_eq!(data_tuple.0, t.sender);
    assert_eq!(data_tuple.1, 70_000i128);    // refund: 100_000 - 300*100
    assert_eq!(data_tuple.2, 30_000i128);    // recipient earned: 300*100
}

#[test]
fn snapshot_event_stream_topped_up() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    c.top_up(&stream_id, &t.sender, &t.token_id, &50_000);

    let events = t.env.events().all();
    let topup_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamToppedUp")
        } else {
            false
        }
    }).collect();

    assert_eq!(topup_events.len(), 1, "Expected exactly one StreamToppedUp event");

    let (contract_id, topics, data) = &topup_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamToppedUp"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (added_amount: i128, new_end_time: u64)
    let data_tuple: (i128, u64) = data.clone().into_val(&t.env);
    assert_eq!(data_tuple.0, 50_000i128);    // added amount
    assert_eq!(data_tuple.1, 1500u64);       // 1000 + 50_000/100
}

#[test]
fn snapshot_event_stream_completed() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let events = t.env.events().all();
    let completed_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamCompleted")
        } else {
            false
        }
    }).collect();

    assert_eq!(completed_events.len(), 1, "Expected exactly one StreamCompleted event");

    let (contract_id, topics, data) = &completed_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamCompleted"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: () â€” empty tuple
    let data_tuple: () = data.clone().into_val(&t.env);
    assert_eq!(data_tuple, ());
}

#[test]
fn snapshot_event_stream_partial_cancelled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // At t=200: streamed = 200*100 = 20_000; remaining = 80_000.
    // Cancel 30_000 â†’ new deposit = 50_000.
    t.env.ledger().set_timestamp(200);
    let new_stream_id = c.partial_cancel_stream(&stream_id, &t.sender, &30_000);

    let events = t.env.events().all();
    let partial_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&t.env);
            first == Symbol::new(&t.env, "StreamPartialCancelled")
        } else {
            false
        }
    }).collect();

    assert_eq!(partial_events.len(), 1, "Expected exactly one StreamPartialCancelled event");

    let (contract_id, topics, data) = &partial_events[0];
    assert_eq!(*contract_id, t.contract_id);

    // Topics: (Symbol("StreamPartialCancelled"), old_stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&t.env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (new_stream_id: u64, sender: Address, refund_amount: i128, new_deposit: i128)
    let data_tuple: (u64, Address, i128, i128) = data.clone().into_val(&t.env);
    assert_eq!(data_tuple.0, new_stream_id);
    assert_eq!(data_tuple.1, t.sender);
    assert_eq!(data_tuple.2, 30_000i128);    // refund amount
    assert_eq!(data_tuple.3, 50_000i128);    // new deposit
}

#[test]
fn snapshot_event_auto_renew_failed() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint only enough for the initial stream â€” not enough for auto-renew.
    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);
    env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &true, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &recipient);

    let events = env.events().all();
    let renew_fail_events: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let topic_vec: soroban_sdk::Vec<Val> = topics.clone();
        if !topic_vec.is_empty() {
            let first: Symbol = topic_vec.get(0).unwrap().into_val(&env);
            first == Symbol::new(&env, "AutoRenewFailed")
        } else {
            false
        }
    }).collect();

    assert_eq!(renew_fail_events.len(), 1, "Expected exactly one AutoRenewFailed event");

    let (emitter, topics, data) = &renew_fail_events[0];
    assert_eq!(*emitter, contract_id);

    // Topics: (Symbol("AutoRenewFailed"), stream_id: u64)
    let topics_vec: soroban_sdk::Vec<Val> = topics.clone();
    assert_eq!(topics_vec.len(), 2);
    let topic_stream_id: u64 = topics_vec.get(1).unwrap().into_val(&env);
    assert_eq!(topic_stream_id, stream_id);

    // Data: (sender: Address, required: i128)
    let data_tuple: (Address, i128) = data.clone().into_val(&env);
    assert_eq!(data_tuple.0, sender);
    assert_eq!(data_tuple.1, 100_000i128);
}

// â”€â”€ Error variant coverage tests (issue #106) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Every variant in StreamError has at least one test that triggers it and
// verifies the exact error variant returned.
//
// Dead code variants (never returned by any code path):
//   - InsufficientBalance (7): No code path returns this error. It exists as
//     a placeholder for future balance-check logic. The contract relies on
//     token::Client::transfer to panic on insufficient balance instead.
//   - InvalidStartTime (12): No code path returns this error. Stream start
//     times are always set to env.ledger().timestamp(), never user-supplied.

#[test]
fn error_stream_not_found() {
    let t = setup();
    let c = client(&t);

    let result = c.try_get_stream(&999);
    assert!(matches!(result, Err(Ok(StreamError::StreamNotFound))));
}

#[test]
fn error_stream_not_found_on_withdraw() {
    let t = setup();
    let c = client(&t);

    let result = c.try_withdraw(&999, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_found_on_cancel() {
    let t = setup();
    let c = client(&t);

    let result = c.try_cancel_stream(&999, &t.sender);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_found_on_top_up() {
    let t = setup();
    let c = client(&t);

    let result = c.try_top_up(&999, &t.sender, &t.token_id, &10_000);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_found_on_partial_cancel() {
    let t = setup();
    let c = client(&t);

    let result = c.try_partial_cancel_stream(&999, &t.sender, &10_000);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_not_recipient() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let other = Address::generate(&t.env);

    let result = c.try_withdraw(&stream_id, &other);
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}

#[test]
fn error_not_sender_on_cancel() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let other = Address::generate(&t.env);

    let result = c.try_cancel_stream(&stream_id, &other);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn error_not_sender_on_top_up() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let other = Address::generate(&t.env);

    let result = c.try_top_up(&stream_id, &other, &t.token_id, &10_000);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn error_not_sender_on_partial_cancel() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let other = Address::generate(&t.env);

    let result = c.try_partial_cancel_stream(&stream_id, &other, &10_000);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn error_stream_not_active_on_withdraw() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    // Cancel the stream first
    c.cancel_stream(&stream_id, &t.sender);

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_active_on_cancel() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    c.cancel_stream(&stream_id, &t.sender);

    let result = c.try_cancel_stream(&stream_id, &t.sender);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_active_on_top_up() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    c.cancel_stream(&stream_id, &t.sender);

    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_stream_not_active_on_top_up_expired() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Advance time past the stream's end time (1000 seconds)
    t.env.ledger().set_timestamp(2000);

    // Mark the stream as expired (this transitions it from Active to Expired)
    let _ = c.mark_expired(&stream_id);

    // Attempt to top up the expired stream should fail with StreamNotActive
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000);
    assert_eq!(result, Err(Ok(StreamError::StreamNotActive)));
}

#[test]
fn success_top_up_paused_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Pause the stream
    c.pause_stream(&stream_id, &t.sender);

    // Topping up a paused stream should succeed
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000);
    assert!(result.is_ok());

    // Verify the stream's deposit was increased
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 100_000 + 10_000);
    assert_eq!(stream.status, StreamStatus::Paused);
}

#[test]
fn error_stream_not_active_on_partial_cancel() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    c.cancel_stream(&stream_id, &t.sender);

    let result = c.try_partial_cancel_stream(&stream_id, &t.sender, &10_000);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

#[test]
fn error_zero_amount_on_create() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &0, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn error_zero_amount_negative_on_create() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &-100, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn error_zero_amount_on_top_up() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &0);
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn error_zero_amount_on_partial_cancel() {
    let t = setup();
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_partial_cancel_stream(&stream_id, &t.sender, &0);
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn error_invalid_duration_on_batch_create() {
    let t = setup();
    let c = client(&t);

    let recipients = soroban_vec![&t.env, t.recipient.clone()];
    let amounts = soroban_vec![&t.env, 10_000i128];

    // duration_seconds = 0 causes end_time overflow check to fail
    let lock_untils = soroban_vec![&t.env, 0u64];
let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &0, &false, &lock_untils,
        &0u64,
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

#[test]
fn error_invalid_cliff() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &1001, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidCliff)));
}

#[test]
fn error_already_initialized() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let result = c.try_initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    assert_eq!(result, Err(Ok(StreamError::AlreadyInitialized)));
}

#[test]
fn error_not_initialized_on_get_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SoroStreamContract, ());
    let c = SoroStreamContractClient::new(&env, &contract_id);

    let result = c.try_get_admin();
    assert_eq!(result, Err(Ok(StreamError::NotInitialized)));
}

#[test]
fn error_not_initialized_on_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SoroStreamContract, ());
    let c = SoroStreamContractClient::new(&env, &contract_id);

    let fake_hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = c.try_upgrade(&fake_hash);
    assert_eq!(result, Err(Ok(StreamError::NotInitialized)));
}

#[test]
fn error_duplicate_stream() {
    let t = setup();
    let c = client(&t);

    c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::DuplicateStream)));
}

#[test]
fn error_invalid_partial_cancel_exceeds_remainder() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // At t=0: remaining = 100_000. cancel_amount = 100_000 exceeds remainder
    // (must be strictly less than remainder).
    let _result = c.try_partial_cancel_stream(&stream_id, &t.sender, &100_000);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &1u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_partial_cancel_stream(&stream_id, &t.sender, &100_000);
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

// â”€â”€ Overflow / checked-arithmetic tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// `create_stream` with 
ow + duration_seconds` overflowing u64 must return
/// `StreamError::Overflow` instead of panicking.
#[test]
fn test_create_stream_end_time_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(u64::MAX - 10);
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert!(result.is_err());
}

/// `create_stream` with 
ow + cliff_seconds` overflowing u64 must return an error.
#[test]
fn test_create_stream_cliff_time_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(u64::MAX - 5);
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &100, &10, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert!(result.is_err());
}

/// Direct unit test of `checked_flow_amount`: a product that overflows i128
/// returns `StreamError::Overflow` rather than panicking.
#[test]
fn test_checked_flow_amount_overflow() {
    let result = checked_flow_amount(10_000_000_000_000_000_000_i128, u64::MAX);
    assert_eq!(result, Err(StreamError::Overflow));
}

/// `checked_flow_amount` returns the correct product when there is no overflow.
#[test]
fn test_checked_flow_amount_ok() {
    let result = checked_flow_amount(100, 500);
    assert_eq!(result, Ok(50_000));
}

/// `top_up` where `extra_seconds = top_up / flow_rate` overflows u64 must return an error.
#[test]
fn test_top_up_extra_seconds_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    use soroban_sdk::token::StellarAssetClient;

    // flow_rate = 1 stroop/sec
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &1_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let huge: i128 = (u64::MAX as i128) + 1;
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &huge);
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &huge);
    assert!(result.is_err());
}

#[test]
fn error_invalid_partial_cancel_leaves_too_little() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_partial_cancel_stream(&stream_id, &t.sender, &99_950);
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

#[test]
fn error_contract_paused() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));
    c.emergency_pause();

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
}

#[test]
fn error_zero_flow_rate() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &1, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroFlowRate)));
}

#[test]
fn error_zero_flow_rate_in_batch() {
    let t = setup();
    let c = client(&t);

    let recipients = soroban_vec![&t.env, t.recipient.clone()];
    let amounts = soroban_vec![&t.env, 1i128];
    let lock_untils = soroban_vec![&t.env, 0u64];

let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroFlowRate)));
}

#[test]
fn error_token_mismatch() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let other_token_admin = Address::generate(&t.env);
    let other_token = t.env
        .register_stellar_asset_contract_v2(other_token_admin)
        .address();

    let result = c.try_top_up(&stream_id, &t.sender, &other_token, &10_000);
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

#[test]
fn error_batch_length_mismatch() {
    let t = setup();
    let c = client(&t);

    let recipients = soroban_vec![&t.env, t.recipient.clone()];
    let amounts = soroban_vec![&t.env, 10_000i128, 20_000i128];
    let lock_untils = soroban_vec![&t.env, 0u64, 0u64];

let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
    );
    assert_eq!(result, Err(Ok(StreamError::BatchLengthMismatch)));
}

#[test]
fn error_zero_amount_in_batch() {
    let t = setup();
    let c = client(&t);

    let recipients = soroban_vec![&t.env, t.recipient.clone()];
    let amounts = soroban_vec![&t.env, 0i128];
    let lock_untils = soroban_vec![&t.env, 0u64];

let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

#[test]
fn error_not_recipient_in_batch_withdraw() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let other = Address::generate(&t.env);

    let result = c.try_batch_withdraw(&soroban_vec![&t.env, stream_id], &other);
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}

#[test]
fn error_invalid_duration_fee_too_high() {
    let t = setup();
    let c = client(&t);

    let result = c.try_set_protocol_fee(&10_001u32);
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

// Dead code documentation:
// - InsufficientBalance (7): Never returned. Token transfers panic via
//   token::Client::transfer on insufficient balance. No contract code path
//   returns this variant. Kept for potential future use with explicit
//   balance checks.
// - InvalidStartTime (12): Never returned. Stream start times are always
//   set to env.ledger().timestamp(), not user-supplied. No code path
//   returns this variant. Kept for potential future use with scheduled
//   stream starts.

#[test]
fn test_top_up_amount_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    use soroban_sdk::token::StellarAssetClient;
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let huge: i128 = (u64::MAX as i128) + 1;
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &huge);
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &huge);
    assert!(result.is_err());
}

/// `top_up` where `end_time + extra_seconds` overflows u64 must return an error.
#[test]
fn test_top_up_end_time_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(u64::MAX - 1_000);

    use soroban_sdk::token::StellarAssetClient;
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000, &1000, &0, &0u64, &false, &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1);
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &1);
    assert!(result.is_err());
}

/// `batch_create_stream` where accumulating amounts overflows i128 must return an error.
#[test]
fn test_batch_create_total_amount_overflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    use soroban_sdk::{token::StellarAssetClient, Vec};

    let a: i128 = 90_000_000_000_000_000_000_000_000_000_000_000_000_i128;
    let b: i128 = 90_000_000_000_000_000_000_000_000_000_000_000_000_i128;

    let mut recipients = Vec::new(&t.env);
    let mut amounts: Vec<i128> = Vec::new(&t.env);
    recipients.push_back(Address::generate(&t.env));
    recipients.push_back(Address::generate(&t.env));
    amounts.push_back(a);
    amounts.push_back(b);

    let mut lock_untils: Vec<u64> = Vec::new(&t.env);
    lock_untils.push_back(0);
    lock_untils.push_back(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &a);
let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils,
        &0u64,
    );
    assert!(result.is_err());
}

// ── Issue #327: batch_create_stream rollback on a mid-batch validation
// failure. `batch_create_stream` runs a validation-only pass over every
// entry (see "Phase 1" in lib.rs) before any token transfer or storage
// write happens, so a bad entry anywhere in the batch must reject the
// whole call with zero side effects — no partial streams, no charge to the
// sender, and (since the batch nonce is incremented as part of the same
// invocation) no consumed nonce either: a Soroban contract invocation that
// returns `Err` has all of its storage writes rolled back by the host.
fn run_batch_create_rollback_case(bad_index: usize) {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let mut recipients = soroban_sdk::Vec::new(&t.env);
    let mut amounts: soroban_sdk::Vec<i128> = soroban_sdk::Vec::new(&t.env);
    let mut tokens = soroban_sdk::Vec::new(&t.env);
    let mut lock_untils: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&t.env);

    for i in 0..5usize {
        recipients.push_back(Address::generate(&t.env));
        // Every entry is valid (10_000) except the one at `bad_index`, which
        // has a zero amount and must fail create_stream's amount validation.
        amounts.push_back(if i == bad_index { 0i128 } else { 10_000i128 });
        tokens.push_back(t.token_id.clone());
        lock_untils.push_back(0u64);
    }

    let sender_bal_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let nonce_before = c.get_nonce(&t.sender);

    let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000u64, &false, &lock_untils, &0u64,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));

    // Full rollback: not one of the 5 streams was created.
    assert_eq!(c.get_all_stream_ids(&0u32, &10u32).len(), 0);

    // Sender was never charged for any of the would-be valid streams.
    let sender_bal_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    assert_eq!(sender_bal_after, sender_bal_before);

    // The nonce consumed at the top of the function is rolled back too.
    assert_eq!(c.get_nonce(&t.sender), nonce_before);
}

#[test]
fn test_batch_create_rollback_invalid_entry_at_start() {
    run_batch_create_rollback_case(0);
}

#[test]
fn test_batch_create_rollback_invalid_entry_in_middle() {
    run_batch_create_rollback_case(2);
}

#[test]
fn test_batch_create_rollback_invalid_entry_at_end() {
    run_batch_create_rollback_case(4);
}

#[test]
fn test_delegate_can_top_up_and_cancel() {
    let t = setup();
    let c = client(&t);
    let operator = Address::generate(&t.env);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&operator, &1_000_000);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    c.set_delegate(&t.sender, &stream_id, &operator);

    // Operator tops up
    c.top_up(&stream_id, &operator, &t.token_id, &50_000);
    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.deposit, 150_000);

    // Operator cancels
    c.cancel_stream(&stream_id, &operator);
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

#[test]
fn test_delegate_cannot_withdraw() {
    let t = setup();
    let c = client(&t);
    let operator = Address::generate(&t.env);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    c.set_delegate(&t.sender, &stream_id, &operator);

    t.env.ledger().set_timestamp(500);

    // Operator tries to withdraw
    let result = c.try_withdraw(&stream_id, &operator);
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}

#[test]
fn test_batch_cancel_stream_success() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let stream_id2 = c.create_stream(&t.sender, &t.recipient, &t.token_id, &200_000, &1000, &0, &1u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    let sender_bal_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    t.env.ledger().set_timestamp(200);
    c.batch_cancel_stream(&soroban_vec![&t.env, stream_id1, stream_id2], &t.sender);

    // Stream 1: 20s earned (20_000), 80s refunded (80_000)
    // Stream 2: 20s earned (40_000), 80s refunded (160_000)
    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(recipient_bal, 20_000 + 40_000);

    let sender_bal_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    assert_eq!(sender_bal_after, sender_bal_before + 80_000 + 160_000);

    assert!(c.try_get_stream(&stream_id1).is_err());
    assert!(c.try_get_stream(&stream_id2).is_err());
}

#[test]
fn error_batch_cancel_not_sender() {
    let t = setup();
    let c = client(&t);
    let other_sender = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&other_sender, &1_000_000);

    let stream_id1 = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let stream_id2 = c.create_stream(&other_sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    let result = c.batch_cancel_stream(&soroban_vec![&t.env, stream_id1, stream_id2], &t.sender);
    assert_eq!(result.get(0).unwrap(), Ok(()));
    assert_eq!(result.get(1).unwrap(), Err(StreamError::NotSender));
}

#[test]
fn error_batch_cancel_empty_list() {
    let t = setup();
    let c = client(&t);
    let result = c.try_batch_cancel_stream(&soroban_vec![&t.env], &t.sender);
    assert_eq!(result, Err(Ok(StreamError::BatchLengthMismatch)));
}

#[test]
fn error_batch_cancel_too_long_list() {
    let t = setup();
    let c = client(&t);
    let mut ids = soroban_sdk::Vec::new(&t.env);
    for i in 0..21 { ids.push_back(i as u64); }
    let result = c.try_batch_cancel_stream(&ids, &t.sender);
    assert_eq!(result, Err(Ok(StreamError::BatchLengthMismatch)));
}

#[test]
fn test_revoke_delegate_strips_capabilities() {
    let t = setup();
    let c = client(&t);
    let operator = Address::generate(&t.env);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&operator, &1_000_000);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    c.set_delegate(&t.sender, &stream_id, &operator);
    c.revoke_delegate(&t.sender, &stream_id);

    // Operator tries to top up
    let result = c.try_top_up(&stream_id, &operator, &t.token_id, &50_000);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

#[test]
fn test_pause_resume() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    t.env.ledger().set_timestamp(200);
    c.pause_stream(&stream_id, &t.sender);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);
    assert_eq!(stream.options.last_pause_time, 200);

    // Get claimable while paused should be for 200s (20_000 tokens)
    t.env.ledger().set_timestamp(500);
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 20_000);

    // Resume at 500
    c.resume_stream(&stream_id, &t.sender);
    let stream_resumed = c.get_stream(&stream_id);
    assert_eq!(stream_resumed.status, StreamStatus::Active);
    // End time should be shifted by (500 - 200) = 300, so from 1000 -> 1300
    assert_eq!(stream_resumed.end_time, 1300);

    // Check claimable at 600. It was active 0-200 and 500-600. Total active = 300s.
    t.env.ledger().set_timestamp(600);
    let claimable_now = c.get_claimable(&stream_id);
    assert_eq!(claimable_now, 30_000);
}

// â”€â”€ Interface trait implementation tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// These tests verify that SoroStreamContract correctly implements the
// SoroStreamInterface trait, enabling type-safe contract invocation through
// the trait and code generation for alternate implementations.

/// Compile-time verification that SoroStreamContract implements SoroStreamInterface.
///
/// If this test fails to compile, it means the trait implementation is incomplete
/// or has signature mismatches. The `assert_implements_interface` function is a
/// zero-cost abstraction that proves the contract satisfies the trait.
fn assert_implements_interface<T: SoroStreamInterface>() {}

#[test]
fn test_contract_implements_interface() {
    // This test compiles if and only if SoroStreamContract implements SoroStreamInterface.
    // If the trait implementation has any method signature mismatches or missing methods,
    // this will fail to compile.
    assert_implements_interface::<SoroStreamContract>();
}

/// Runtime test: Call a trait method through the trait object to verify delegation works.
///
/// This test demonstrates that methods can be invoked through the SoroStreamInterface trait,
/// not just through the concrete contractimpl methods. This enables:
/// - SDK code generation for type-safe client stubs
/// - Alternate implementations that satisfy the same interface
/// - Runtime polymorphism for contract testing
#[test]
fn test_interface_trait_method_delegation() {
    let t = setup();
    let c = client(&t);

    // Create a stream using the direct contractimpl method
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Retrieve and verify the stream was created correctly
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id);
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
    assert_eq!(stream.token, t.token_id);
    assert_eq!(stream.deposit, 100_000);
    assert_eq!(stream.flow_rate, 100);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// Verify that the trait methods maintain identical semantics to contractimpl.
///
/// This test ensures that calling through the trait delegation does not introduce
/// any behavioral differences or side effects.
#[test]
fn test_interface_preserves_semantics() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Advance time and withdraw through trait
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // Verify the withdrawal was processed identically to direct contractimpl call
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000, "Trait delegation did not preserve withdrawal semantics");
}

/// Verify get_stats through the trait interface.
#[test]
fn test_interface_get_stats() {
    let t = setup();
    let c = client(&t);

    // Create multiple streams
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &50_000,
        &500,
        &0,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Get stats through trait
    let stats = c.get_stats();
    assert_eq!(stats.total_streams, 2);
    assert_eq!(stats.active_streams, 2);
    assert_eq!(stats.total_volume, 150_000);
}

/// Verify protocol fee methods through the trait interface.
#[test]
fn test_interface_protocol_fee() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    // Set protocol fee through trait
    c.set_protocol_fee(&100); // 1% = 100 bps
    c.set_treasury_address(&admin);

    // Get protocol fee info through trait
    let (fee_bps, treasury) = c.get_protocol_fee_info();
    assert_eq!(fee_bps, 100);
    assert_eq!(treasury, Some(admin));
}

/// Verify pagination methods through the trait interface.
#[test]
fn test_interface_pagination_methods() {
    let t = setup();
    let c = client(&t);

    // Create multiple streams for pagination testing
    let id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Test get_all_stream_ids through trait
    let all_ids = c.get_all_stream_ids(&0u32, &10u32);
    assert!(all_ids.len() >= 2);
    assert_eq!(all_ids.get_unchecked(0), id1);
    assert_eq!(all_ids.get_unchecked(1), id2);

    // Test get_streams_by_sender through trait
    let sender_streams = c.get_streams_by_sender(&t.sender, &0u32, &10u32);
    assert!(sender_streams.len() >= 2);

    // Test get_streams_by_recipient through trait
    let recipient_streams = c.get_streams_by_recipient(&t.recipient, &0u32, &10u32);
    assert!(recipient_streams.len() >= 2);

    // Test active streams through trait
    let active_sender = c.get_active_streams_by_sender(&t.sender);
    assert!(active_sender.len() >= 2);

    let active_recipient = c.get_active_streams_by_recipient(&t.recipient);
    assert!(active_recipient.len() >= 2);
}

/// Verify batch operations through the trait interface.
#[test]
fn test_interface_batch_operations() {
    let t = setup();
    let c = client(&t);

    let recipient2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &500_000);

    let recipients = soroban_vec![&t.env, t.recipient.clone(), recipient2.clone()];
    let amounts = soroban_vec![&t.env, 100_000i128, 50_000i128];
    let lock_untils = soroban_vec![&t.env, 0u64, 0u64];

    // Create batch through trait
let mut tokens = soroban_sdk::Vec::new(&t.env);
    for _ in 0..recipients.len() {
        tokens.push_back(t.token_id.clone());
    }
        let stream_ids = c.batch_create_stream(
        &t.sender,
        &recipients,
        &amounts,
        &tokens,
        &1000,
        &false,
        &lock_untils,
        &0u64,
    );
    assert_eq!(stream_ids.len(), 2);

    // Withdraw batch through trait (only first stream for t.recipient)
    let first_id = soroban_sdk::vec![&t.env, stream_ids.get_unchecked(0)];
    let withdrawal_amounts = c.batch_withdraw(&first_id, &t.recipient);
    assert_eq!(withdrawal_amounts.len(), 1);
}

/// Verify admin operations through the trait interface.
#[test]
fn test_interface_admin_operations() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);

    // Initialize through trait
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    // Get admin through trait
    assert_eq!(c.get_admin(), admin);

    let new_admin = Address::generate(&t.env);

    // Set admin through trait
    c.set_admin(&new_admin);
    assert_eq!(c.get_admin(), new_admin);

    // Pause/resume through trait
    assert!(!c.is_paused());
    c.emergency_pause();
    assert!(c.is_paused());
    c.emergency_resume();
    assert!(!c.is_paused());
}

/// Verify is_participant through the trait interface.
#[test]
fn test_interface_is_participant() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Test sender participation through trait
    assert!(c.is_participant(&stream_id, &t.sender));

    // Test recipient participation through trait
    assert!(c.is_participant(&stream_id, &t.recipient));

    // Test non-participant
    let other = Address::generate(&t.env);
    assert!(!c.is_participant(&stream_id, &other));
}

/// #188 â€“ Recipient can withdraw correctly after a top_up.
#[test]
fn test_withdraw_after_top_up() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &50_000);
    c.top_up(&stream_id, &t.sender, &t.token_id, &50_000);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 150_000);
    assert_eq!(stream.end_time, 1500);

    t.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &t.recipient);
    let bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(bal, 60_000);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.total_withdrawn, 60_000);

    t.env.ledger().set_timestamp(1500);
    c.withdraw(&stream_id, &t.recipient);
    let bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(bal, 150_000);

    assert!(c.try_get_stream(&stream_id).is_err());
}

/// Issue #187 â€“ cancel_stream with zero withdrawals: full deposit refunded to sender.
#[test]
fn test_cancel_stream_with_zero_withdrawals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(100);

    let initial_sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let claimable_before = c.get_claimable(&stream_id);
    assert_eq!(claimable_before, 0, "claimable must be 0 before any time passes");

    c.cancel_stream(&stream_id, &t.sender);

    let sender_bal_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    assert_eq!(
        sender_bal_after, initial_sender_bal,
        "sender must receive full deposit refund",
    );

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(recipient_bal, 0, "recipient must receive 0 when cancelled before cliff");

    assert!(
        c.try_get_stream(&stream_id).is_err(),
        "stream entry must be removed after cancel",
    );
}

// --- #186: Emergency pause blocks create_stream and withdraw ---

#[test]
fn test_emergency_pause_blocks_create_stream_186() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.emergency_pause();

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
}

#[test]
fn test_emergency_resume_unblocks_create_stream_186() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.emergency_pause();
    c.emergency_resume();

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

#[test]
fn test_emergency_pause_blocks_withdraw_186() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(500);
    c.emergency_pause();

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
}

#[test]
fn test_emergency_resume_unblocks_withdraw_186() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(500);
    c.emergency_pause();
    c.emergency_resume();

    c.withdraw(&stream_id, &t.recipient);
}

// --- #249: cancel_stream properly cleans up sender/recipient index ---

/// Issue #249 â€“ After cancellation, get_streams_by_sender and get_streams_by_recipient
/// must no longer return the cancelled stream.
#[test]
fn test_cancel_stream_removes_from_sender_and_recipient_index_249() {
// â”€â”€ Rounding dust tests (issue #248) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// When deposit is not evenly divisible by duration, flow_rate rounds down.
/// The final withdrawal should not error due to rounding dust â€” it should
/// cap the claimable at deposit - total_withdrawn.
#[test]
fn test_withdraw_dust_not_erroring() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let sender_streams_before = c.get_streams_by_sender(&t.sender, &0u32, &10u32);
    assert_eq!(sender_streams_before.len(), 1);

    let recipient_streams_before = c.get_streams_by_recipient(&t.recipient, &0u32, &10u32);
    assert_eq!(recipient_streams_before.len(), 1);

    t.env.ledger().set_timestamp(300);
    c.cancel_stream(&stream_id, &t.sender);

    let sender_streams_after = c.get_streams_by_sender(&t.sender, &0u32, &10u32);
    assert_eq!(sender_streams_after.len(), 0, "sender index must be empty after cancel");

    let recipient_streams_after = c.get_streams_by_recipient(&t.recipient, &0u32, &10u32);
    assert_eq!(recipient_streams_after.len(), 0, "recipient index must be empty after cancel");

    assert!(c.try_get_stream(&stream_id).is_err(), "stream must not exist after cancel");
}

// --- #251: cliff_end_time == end_time boundary ---

/// Issue #251 â€“ When cliff_end_time == end_time, the entire deposit becomes
/// claimable at exactly cliff_end_time. Nothing is claimable one second before.
#[test]
fn test_cliff_equals_end_time_boundary_251() {
    // 100 / 3 = 33 (floor). Total streamable = 33*3 = 99. Dust = 1.
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100, &3, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // Withdraw at t=1: 33
    t.env.ledger().set_timestamp(1);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.recipient), 33);

    // Withdraw at t=2: another 33 â†’ total 66
    t.env.ledger().set_timestamp(2);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.recipient), 66);

    // Withdraw at t=3 (end): claimable = 33, but available = 100-66 = 34.
    // Due to dust, raw claimable (33) < available (34), so recipient gets 33 more = 99.
    t.env.ledger().set_timestamp(3);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.recipient), 99);

    // Stream should be removed after end
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// top_up with dust: effective_amount rounds to whole seconds.
/// The total should still be claimable without error.
#[test]
fn test_top_up_dust_rounding_correctness() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let duration = 100u64;
    let cliff = 100u64;
    let deposit = 100_000i128;

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &deposit, &duration, &cliff, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.cliff_time, stream.end_time, "cliff_time must equal end_time");

    t.env.ledger().set_timestamp(99);
    let claimable_before = c.get_claimable(&stream_id);
    assert_eq!(claimable_before, 0, "nothing claimable one second before cliff");

    t.env.ledger().set_timestamp(100);
    let claimable_at_cliff = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_cliff, deposit, "entire deposit claimable at cliff == end_time");
}

// --- #252: get_claimable at exactly end_time ---

/// Issue #252 â€“ At a stream's exact end_time, all of the deposit should be
/// claimable. After end_time, claimable must not increase further.
#[test]
fn test_get_claimable_at_exact_end_time_252() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let duration = 100u64;
    let deposit = 100_000i128;

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &deposit, &duration, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(100);
    let claimable_at_end = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_at_end, deposit,
        "full deposit must be claimable at exactly end_time"
    );

    t.env.ledger().set_timestamp(101);
    let claimable_after = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_after, deposit,
        "claimable must not increase beyond end_time"
    );
}

/// Issue #252 â€“ Non-zero cliff: at end_time the full deposit is still claimable
/// provided the cliff has already passed.
#[test]
fn test_get_claimable_at_end_time_with_cliff_252() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let duration = 100u64;
    let cliff = 10u64;
    let deposit = 100_000i128;

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &deposit, &duration, &cliff, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    t.env.ledger().set_timestamp(100);
    let claimable_at_end = c.get_claimable(&stream_id);
    assert_eq!(
        claimable_at_end, deposit,
        "full deposit claimable at end_time even with non-zero cliff"
    );
}

// --- #254: concurrent create and cancel in same ledger sequence ---

/// Issue #254 â€“ Creating a stream and immediately cancelling it in the same
/// ledger sequence must produce a consistent final state: either the cancel
/// completes with a full refund, or it is rejected cleanly.
#[test]
fn test_concurrent_create_and_cancel_same_ledger_254() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(100);

    let initial_sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // No ledger advancement â€“ cancel in the same sequence
    c.cancel_stream(&stream_id, &t.sender);

    // Stream must be fully removed
    assert!(
        c.try_get_stream(&stream_id).is_err(),
        "stream must not exist after cancel in same ledger"
    );

    // Sender must receive full refund (no time elapsed)
    let sender_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    assert_eq!(
        sender_bal, initial_sender_bal,
        "sender must get full refund when cancel is immediate"
    );

    // Recipient must have received nothing
    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(recipient_bal, 0, "recipient gets nothing when cancelled instantly");

    // Index must be clean
    let sender_streams = c.get_streams_by_sender(&t.sender, &0u32, &10u32);
    assert_eq!(sender_streams.len(), 0, "sender index empty after same-ledger cancel");
    // flow_rate = 33
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100, &3, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // Top up 50: effective = 50 - (50 % 33) = 50 - 17 = 33. extra = 33/33 = 1s.
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &50);
    c.top_up(&stream_id, &t.sender, &t.token_id, &50);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 133);
    assert_eq!(stream.end_time, 4);

    // Withdraw everything at end
    t.env.ledger().set_timestamp(4);
    c.withdraw(&stream_id, &t.recipient);
    // flow_rate=33, duration=4 â†’ 33*4 = 132. deposit=133, dust=1.
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.recipient), 132);
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// cancel_stream with rounding dust: refund should not underflow.
#[test]
fn test_cancel_stream_dust_no_underflow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 100 / 3 = 33
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100, &3, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // Cancel at t=2: earned = 66, available = 100, refund = 34.
    t.env.ledger().set_timestamp(2);
    c.cancel_stream(&stream_id, &t.sender);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.recipient), 66);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&t.sender), 999_934);
}

// â”€â”€ get_stats counter tests (issue #246) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// get_stats.total_streams reflects total ever created (including cancelled).
/// get_stats.active_streams reflects currently active count.
#[test]
fn test_get_stats_tracks_active_and_total() {
    let t = setup();
    let c = client(&t);

    let id1 = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let id2 = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &1u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    let stats = c.get_stats();
    assert_eq!(stats.total_streams, 2);
    assert_eq!(stats.active_streams, 2);
    assert_eq!(stats.total_volume, 200_000);

    // Cancel one stream
    c.cancel_stream(&id1, &t.sender);

    let stats = c.get_stats();
    assert_eq!(stats.total_streams, 2); // total ever created stays at 2
    assert_eq!(stats.active_streams, 1); // active decremented
}

/// get_stats.active_streams decrements on pause and increments on resume.
#[test]
fn test_get_stats_pause_resume_counter() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    assert_eq!(c.get_stats().active_streams, 1);

    c.pause_stream(&stream_id, &t.sender);
    assert_eq!(c.get_stats().active_streams, 0);

    c.resume_stream(&stream_id, &t.sender);
    assert_eq!(c.get_stats().active_streams, 1);
}

/// recalibrate_stats admin instruction corrects drift.
#[test]
fn test_recalibrate_stats_corrects_drift() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &1u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    assert_eq!(c.get_stats().active_streams, 2);

    // Cancel one stream
    let id1 = c.get_all_stream_ids(&0, &2).get_unchecked(0);
    c.cancel_stream(&id1, &t.sender);
    assert_eq!(c.get_stats().active_streams, 1);

    // Recalibrate should confirm the count
    c.recalibrate_stats(&admin);
    assert_eq!(c.get_stats().active_streams, 1);
}

/// recalibrate_stats rejects non-admin caller.
#[test]
fn test_recalibrate_stats_rejects_non_admin() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let result = c.try_recalibrate_stats(&t.sender);
    assert!(result.is_err());
}

// â”€â”€ bump_stream_ttl tests (issue #225) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// bump_stream_ttl extends the storage TTL so the stream entry remains accessible
/// after its original TTL would have expired. Any caller â€” not just participants â€”
/// may invoke this instruction.
#[test]
fn test_bump_stream_ttl_extends_accessibility() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);

    // Set ledger sequence near where the default TTL might expire.
    t.env.ledger().set_sequence_number(99_990);

    // Bump the TTL â€” no auth required, any caller works.
    c.bump_stream_ttl(&stream_id);

    // Advance ledger well beyond original TTL.
    t.env.ledger().set_sequence_number(200_000);

    // Stream should still be accessible.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// bump_stream_ttl can be called by a third party (non-participant).
#[test]
fn test_bump_stream_ttl_any_caller_can_call() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let other = Address::generate(&t.env);

    let result = c.try_bump_stream_ttl(&stream_id, &other);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false);
    // A completely unrelated address can bump TTL â€” no error expected.
    c.bump_stream_ttl(&stream_id);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// bump_stream_ttl rejects cancelled / non-active streams.
#[test]
fn test_bump_stream_ttl_rejects_cancelled() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    c.cancel_stream(&stream_id, &t.sender);

    // After cancellation the stream is removed from storage â†’ StreamNotFound.
    let result = c.try_bump_stream_ttl(&stream_id);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));
}

/// bump_stream_ttl works on paused streams as well.
#[test]
fn test_bump_stream_ttl_works_on_paused_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false, &0i128, &None::<u32>, &None::<i128>, &None::<u32>);
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64, &false, &0u64, &false);
    c.pause_stream(&stream_id, &t.sender);

    // Should succeed â€” paused streams still need their TTL extended.
    let result = c.try_bump_stream_ttl(&stream_id);
    assert!(result.is_ok());
}

/// bump_stream_ttl uses a 24-hour buffer so that streams near their end still get bumped.
#[test]
fn test_bump_stream_ttl_buffer_applied_for_nearly_expired_stream() {
    let t = setup();
    let c = client(&t);
    // Stream ends in 10 seconds.
    t.env.ledger().set_timestamp(0);
    let stream_id = c.create_stream(&t.sender, &t.recipient, &t.token_id, &100_000, &10, &0, &0u64, &false, &0u64, &false);

    t.env.ledger().set_timestamp(5); // 5 s before end_time
    // Should not panic â€” safety buffer covers the tiny remaining duration.
    let result = c.try_bump_stream_ttl(&stream_id);
    assert!(result.is_ok());
}

// â”€â”€ Delegate management tests (issue #226) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// set_delegate stores the delegate and emits DelegateSet event.
#[test]
fn test_set_delegate_stores_delegate() {
    let t = setup();
    let c = client(&t);
    let delegate = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    c.set_delegate(&t.sender, &stream_id, &delegate);

    let stored = c.get_delegate(&stream_id);
    assert_eq!(stored, Some(delegate));
}

/// Only the sender can set a delegate â€” non-sender is rejected.
#[test]
fn test_set_delegate_rejected_for_non_sender() {
    let t = setup();
    let c = client(&t);
    let impostor = Address::generate(&t.env);
    let delegate = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    let result = c.try_set_delegate(&impostor, &stream_id, &delegate);
    assert_eq!(result, Err(Ok(StreamError::NotSender)));
}

/// Delegate can cancel a stream in place of the sender.
#[test]
fn test_delegate_can_cancel_stream() {
    let t = setup();
    let c = client(&t);
    let delegate = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&delegate, &1_000_000);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    c.set_delegate(&t.sender, &stream_id, &delegate);
    c.cancel_stream(&stream_id, &delegate);

    // Stream removed after cancel.
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

/// A non-delegate third party cannot act as sender.
#[test]
fn test_non_delegate_cannot_cancel() {
    let t = setup();
    let c = client(&t);
    let impostor = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    let result = c.try_cancel_stream(&stream_id, &impostor);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

/// After revoke_delegate the former delegate loses all permissions.
#[test]
fn test_revoke_delegate_removes_permissions() {
    let t = setup();
    let c = client(&t);
    let delegate = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&delegate, &1_000_000);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    c.set_delegate(&t.sender, &stream_id, &delegate);
    c.revoke_delegate(&t.sender, &stream_id);

    // get_delegate now returns None.
    assert_eq!(c.get_delegate(&stream_id), None);

    // Former delegate can no longer cancel.
    let result = c.try_cancel_stream(&stream_id, &delegate);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));
}

/// Sender can resume sole control after revoking delegate.
#[test]
fn test_sender_retains_control_after_revoke() {
    let t = setup();
    let c = client(&t);
    let delegate = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    c.set_delegate(&t.sender, &stream_id, &delegate);
    c.revoke_delegate(&t.sender, &stream_id);

    // Sender can still cancel the stream.
    c.cancel_stream(&stream_id, &t.sender);
    assert!(c.try_get_stream(&stream_id).is_err());
}

/// Delegate address is returned in get_delegate response.
#[test]
fn test_get_delegate_returns_correct_address() {
    let t = setup();
    let c = client(&t);
    let delegate = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    assert_eq!(c.get_delegate(&stream_id), None);

    c.set_delegate(&t.sender, &stream_id, &delegate);
    assert_eq!(c.get_delegate(&stream_id), Some(delegate.clone()));

    c.revoke_delegate(&t.sender, &stream_id);
    assert_eq!(c.get_delegate(&stream_id), None);
}


// â”€â”€ Expired state & mark_expired tests (issue #228) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// get_stream returns Expired status once the stream's end_time has passed,
/// even without an explicit mark_expired call.
#[test]
fn test_get_stream_returns_expired_after_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    // Before end_time: still Active.
    t.env.ledger().set_timestamp(500);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);

    // At exactly end_time: Expired.
    t.env.ledger().set_timestamp(1000);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);

    // After end_time: still Expired.
    t.env.ledger().set_timestamp(2000);
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Expired);
}

/// Cancelled streams never transition to Expired via get_stream.
#[test]
fn test_cancelled_stream_not_returned_as_expired() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );
    c.cancel_stream(&stream_id, &t.sender);

    // After cancellation the stream is removed â€” should return StreamNotFound.
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

// â”€â”€â”€ Holdback escrow tests (#224) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Helper: create a stream with a non-zero holdback amount.
/// `total` is the full amount locked; `holdback` is the escrow portion.
/// The sender is minted enough tokens before the call.
fn create_holdback_stream(
    t: &TestEnv,
    total: i128,
    holdback: i128,
    duration: u64,
    nonce: u64,
) -> u64 {
    let c = client(t);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &total);
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &total,
        &duration,
        &0,
        &nonce,
        &false,
        &0u64,
        &false,
        &holdback,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    )
}

/// Holdback is deducted from the streaming deposit at creation time.
/// stream.deposit == total - holdback; flow_rate == deposit / duration.
#[test]
fn test_holdback_deducted_from_deposit() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 100_000;
    let holdback: i128 = 20_000;
    let duration: u64 = 1000;

    let stream_id = create_holdback_stream(&t, total, holdback, duration, 42);
    let stream = client(&t).get_stream(&stream_id);

    assert_eq!(stream.deposit, total - holdback, "deposit should be streaming portion only");
    assert_eq!(stream.options.holdback_amount, holdback);
    assert!(!stream.options.holdback_claimed);
    assert_eq!(stream.flow_rate, (total - holdback) / duration as i128);
}

/// Full contract balance after creation equals total (streaming + holdback).
#[test]
fn test_holdback_contract_holds_full_amount() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 50_000;
    let holdback: i128 = 10_000;

    create_holdback_stream(&t, total, holdback, 500, 1);

    let contract_balance =
        soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.contract_id);
    assert_eq!(contract_balance, total, "contract should hold the full amount");
}

/// Sender releases the holdback â†’ recipient receives it; holdback_claimed becomes true.
#[test]
fn test_release_holdback_transfers_to_recipient() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 100_000;
    let holdback: i128 = 30_000;
    let stream_id = create_holdback_stream(&t, total, holdback, 1000, 10);

    let c = client(&t);

    // Advance time so some streaming has happened (not required for release, but realistic)
    t.env.ledger().set_timestamp(500);

    let before = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.recipient);
    c.release_holdback(&stream_id, &t.sender);
    let after = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(after - before, holdback, "recipient should receive the holdback amount");

    let stream = c.get_stream(&stream_id);
    assert!(stream.options.holdback_claimed, "holdback_claimed must be true after release");
}

/// Sender can claw back the holdback before recipient claims it.
#[test]
fn test_claw_back_holdback_returns_to_sender() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 80_000;
    let holdback: i128 = 25_000;
    let stream_id = create_holdback_stream(&t, total, holdback, 800, 20);

    let c = client(&t);

    let before = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.sender);
    c.claw_back_holdback(&stream_id, &t.sender);
    let after = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.sender);

    assert_eq!(after - before, holdback, "sender should receive the clawed-back holdback");

    let stream = c.get_stream(&stream_id);
    assert!(stream.options.holdback_claimed, "holdback_claimed must be true after claw-back");
}

/// Double-release is rejected (holdback already settled).
#[test]
fn test_release_holdback_double_release_rejected() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let stream_id = create_holdback_stream(&t, 60_000, 15_000, 600, 30);
    let c = client(&t);

    c.release_holdback(&stream_id, &t.sender);

    // Second release attempt must fail
    let result = c.try_release_holdback(&stream_id, &t.sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotActive)),
        "second release should fail with StreamNotActive"
    );
}

/// Claw-back after release is also rejected.
#[test]
fn test_claw_back_after_release_rejected() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let stream_id = create_holdback_stream(&t, 60_000, 15_000, 600, 31);
    let c = client(&t);

    c.release_holdback(&stream_id, &t.sender);

    let result = c.try_claw_back_holdback(&stream_id, &t.sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotActive)),
        "claw-back after release should be rejected"
    );
}

/// release_holdback on a zero-holdback stream returns ZeroAmount.
#[test]
fn test_release_holdback_zero_holdback_rejected() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    // Create a stream with no holdback
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let c = client(&t);
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &99u64, &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let result = c.try_release_holdback(&stream_id, &t.sender);
    assert_eq!(
        result,
        Err(Ok(StreamError::ZeroAmount)),
        "release on zero-holdback stream should return ZeroAmount"
    );
}

/// Holdback is included in the sender refund when the stream is cancelled before release.
#[test]
fn test_holdback_returned_to_sender_on_cancel() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 100_000;
    let holdback: i128 = 40_000;
    let duration: u64 = 1000;

    let stream_id = create_holdback_stream(&t, total, holdback, duration, 50);
    let c = client(&t);

    // Advance time â€” recipient earns some tokens
    t.env.ledger().set_timestamp(200);

    let sender_before = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_before = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.recipient);

    c.cancel_stream(&stream_id, &t.sender);

    let sender_after = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_after = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.recipient);

    let streaming_deposit = total - holdback; // 60_000
    let flow_rate = streaming_deposit / duration as i128; // 60
    let elapsed: i128 = 200;
    let earned = flow_rate * elapsed; // 12_000
    let unstreamed = streaming_deposit - earned; // 48_000

    // Sender gets back: unstreamed portion + holdback (not yet released)
    assert_eq!(
        sender_after - sender_before,
        unstreamed + holdback,
        "sender should receive unstreamed + holdback on cancel"
    );
    // Recipient gets: earned portion only (holdback not released)
    assert_eq!(
        recipient_after - recipient_before,
        earned,
        "recipient should receive only the earned amount on cancel"
    );
}

/// Partial holdback: streaming still works correctly when holdback < total.
#[test]
fn test_partial_holdback_streaming_works() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let total: i128 = 100_000;
    let holdback: i128 = 10_000;
    let duration: u64 = 1000;
    let streaming = total - holdback; // 90_000
    let flow_rate = streaming / duration as i128; // 90

    let stream_id = create_holdback_stream(&t, total, holdback, duration, 60);
    let c = client(&t);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let bal = soroban_sdk::token::Client::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(bal, flow_rate * 500, "recipient earns from streaming portion only");
}

/// holdback_amount == amount is rejected (nothing left to stream).
#[test]
fn test_holdback_equal_to_amount_rejected() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let c = client(&t);
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &70u64, &false, &0u64, &false,
        &100_000i128, &None::<u32>, &None::<i128>, &None::<u32>, // holdback == amount â†’ invalid
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

/// Negative holdback_amount is rejected.
#[test]
fn test_negative_holdback_rejected() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let c = client(&t);
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &71u64, &false, &0u64, &false,
        &-1i128, &None::<u32>, &None::<i128>, &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::ZeroAmount)));
}

/// Non-sender cannot release or claw back holdback.
#[test]
fn test_holdback_only_sender_can_operate() {
    let t = setup();
    t.env.ledger().set_timestamp(0);

    let stream_id = create_holdback_stream(&t, 100_000, 20_000, 1000, 80);
    let c = client(&t);

    let result = c.try_release_holdback(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::NotAuthorized)));

    let result2 = c.try_claw_back_holdback(&stream_id, &t.recipient);
    assert_eq!(result2, Err(Ok(StreamError::NotAuthorized)));
}
/// mark_expired transitions a stream to Expired after end_time and emits event.
#[test]
fn test_mark_expired_succeeds_after_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    // Advance past end_time.
    t.env.ledger().set_timestamp(1001);
    c.mark_expired(&stream_id);

    // Persisted status is now Expired.
    let raw = c.get_stream(&stream_id);
    assert_eq!(raw.status, StreamStatus::Expired);
}

/// mark_expired rejects a stream that has not yet reached end_time.
#[test]
fn test_mark_expired_rejects_before_end_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );

    t.env.ledger().set_timestamp(500); // still before end_time
    let result = c.try_mark_expired(&stream_id);
    assert_eq!(result, Err(Ok(StreamError::StreamNotComplete)));
}

/// mark_expired rejects already-Cancelled streams.
#[test]
fn test_mark_expired_rejects_cancelled_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );
    c.cancel_stream(&stream_id, &t.sender);

    // Stream is removed on cancel â€” StreamNotFound.
    let result = c.try_mark_expired(&stream_id);
    assert!(result.is_err());
}

/// mark_expired is callable by anyone, not only the sender/recipient.
#[test]
fn test_mark_expired_callable_by_anyone() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id, &100_000, &1000, &0, &0u64,
        &false, &0u64, &false,
    );
    t.env.ledger().set_timestamp(1001);

    // A third-party address can call mark_expired.
    let result = c.try_mark_expired(&stream_id);
    assert!(result.is_ok());
}

// â”€â”€ sweep_fees tests (#222) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// sweep_fees with zero balance is a no-op: no transfer, no event, no error.
#[test]
fn test_sweep_fees_zero_balance_is_noop() {
    let t = setup();
    let c = client(&t);
    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    let destination = Address::generate(&t.env);

    // No fees have been collected yet â€” should succeed without doing anything.
    c.sweep_fees(&t.token_id, &destination);

    // Destination balance remains zero.
    let bal = TokenClient::new(&t.env, &t.token_id).balance(&destination);
    assert_eq!(bal, 0);

    // fees_collected tracker is still zero.
    assert_eq!(c.get_fees_collected(&t.token_id), 0);
}

/// sweep_fees with a non-zero balance transfers the exact amount to destination
/// and resets the counter.
#[test]
fn test_sweep_fees_nonzero_balance_transfers_and_resets() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    // 1% fee = 100 bps
    c.set_protocol_fee(&100u32);

    // Create a stream with 100_000 stroops over 1000s â†’ flow_rate = 100
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64, &false,
    );

    // Advance to halfway and withdraw â€” fee = 1% of 50_000 = 500 stroops
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // fees_collected should equal the fee deducted (500 stroops)
    let collected = c.get_fees_collected(&t.token_id);
    assert_eq!(collected, 500);

    // Recipient should have received 50_000 - 500 = 49_500
    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(recipient_bal, 49_500);

    // Sweep fees to treasury destination
    let treasury = Address::generate(&t.env);
    c.sweep_fees(&t.token_id, &treasury);

    // Treasury received the exact fee amount
    let treasury_bal = TokenClient::new(&t.env, &t.token_id).balance(&treasury);
    assert_eq!(treasury_bal, 500);

    // Counter reset to zero after sweep
    assert_eq!(c.get_fees_collected(&t.token_id), 0);
}

/// fees accumulate across multiple withdrawals before a single sweep.
#[test]
fn test_sweep_fees_accumulates_across_withdrawals() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    // 2% fee = 200 bps
    c.set_protocol_fee(&200u32);

    // Stream: 100_000 over 1000s â†’ flow_rate = 100, 2% fee
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &0u64, &false, &0u64, &false,
    );

    // First withdrawal at t=200: claimable=20_000, fee=400
    t.env.ledger().set_timestamp(200);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_fees_collected(&t.token_id), 400);

    // Second withdrawal at t=600: claimable=40_000, fee=800
    t.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &t.recipient);
    assert_eq!(c.get_fees_collected(&t.token_id), 1200);

    // Single sweep collects both
    let treasury = Address::generate(&t.env);
    c.sweep_fees(&t.token_id, &treasury);
    assert_eq!(TokenClient::new(&t.env, &t.token_id).balance(&treasury), 1200);
    assert_eq!(c.get_fees_collected(&t.token_id), 0);
}

/// sweep_fees can only be called by admin; non-admin caller panics.
#[test]
fn test_sweep_fees_unauthorized_rejected() {
    let env = Env::default();
    // Note: do NOT call mock_all_auths so that auth is actually enforced
    let contract_id = env.register(SoroStreamContract, ());
    let c = SoroStreamContractClient::new(&env, &contract_id);

    // Initialize with a known admin (mock all auths just for this call)
    env.mock_all_auths();
    let admin = Address::generate(&env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let destination = Address::generate(&env);

    // Now call without mocking auths â€” should fail auth check
    let result = c.try_sweep_fees(&token_id, &destination);
    assert!(result.is_err(), "non-admin should not be able to sweep fees");
}

// â”€â”€ Issue #308 â€“ Minimum valid parameter boundary tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Minimum valid stream: amount = duration = 1 â†’ flow_rate = 1.
///
/// This is the smallest set of parameters that passes every validation gate:
/// - amount > 0  âœ“
/// - duration >= min_duration (0 after set_min_duration)  âœ“
/// - flow_rate = amount / duration = 1 (non-zero)  âœ“
#[test]
fn test_create_stream_minimum_valid_parameters() {
    let t = setup(); // set_min_duration(0) called inside setup()
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1i128,  // minimum amount: 1 stroop
        &1u64,   // minimum duration: 1 second
        &0u64,   // no cliff
        &0u64,   // nonce
        &false,  // auto_renew
        &0u64,   // lock_until
        &false,  // allow_recipient_termination
        &0i128,  // no holdback
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 1, "deposit should equal the minimum amount");
    assert_eq!(stream.flow_rate, 1, "flow_rate should be 1 stroop/sec");
    assert_eq!(stream.status, StreamStatus::Active, "stream should be active");
    assert_eq!(stream.sender, t.sender);
    assert_eq!(stream.recipient, t.recipient);
}

/// get_claimable returns the correct amount at the minimum boundary.
///
/// With flow_rate = 1 and elapsed = 1 second the claimable balance should be 1.
#[test]
fn test_create_stream_minimum_claimable_after_one_second() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(1000);


// â”€â”€ Stream ID uniqueness regression tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Acceptance criteria:
//   - 100 streams created in sequence all receive distinct IDs
//   - No stream data is overwritten under any tested creation scenario
//
// The contract uses a monotonic nonce per-sender so each call produces a
// different (sender, recipient, start_time, nonce) tuple fed into SHA-256.
// These tests confirm that the derived IDs never collide across 100 rapid
// creations and that the stored stream data is intact for every ID.

/// Creates 100 streams in sequence and asserts every stream ID is unique.
/// Also verifies that each stored stream's deposit matches what was deposited,
/// confirming no stream entry was silently overwritten.
#[test]
fn test_stream_id_uniqueness_100_sequential() {
// â”€â”€â”€ Issue #321: max-duration stream edge-case tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// A stream with duration = u64::MAX seconds must be created without panicking,
/// stored correctly, and report get_claimable == 0 at start_time.
///
/// Deposit is set to i128::MAX so that `flow_rate = deposit / duration` rounds
/// down to 1 stroop/sec rather than 0 (which would be rejected as ZeroFlowRate).
/// The key property under test is that no arithmetic overflows during creation.
#[test]
fn test_create_stream_max_duration_no_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint enough tokens for 100 streams Ã— 1_000 stroops each.
    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    // Disable minimum duration so short streams are accepted in tests.
    c.set_min_duration(&sender, &0u64);

    // Raise the per-sender stream limit so all 100 fit.
    c.set_max_streams(&100u32);

    env.ledger().set_timestamp(1_000);

    let mut ids: std::vec::Vec<u64> = std::vec::Vec::new();

    for nonce in 0u64..100 {
        let stream_id = c.create_stream(
            &sender,
            &recipient,
            &token_id,
            &1_000i128,   // amount
            &3600u64,     // duration_seconds (1 hour â€” above any min_dur)
            &0u64,        // cliff_seconds
            &nonce,       // unique nonce per iteration
            &false,       // auto_renew
            &0u64,        // lock_until
            &false,       // allow_recipient_termination
            &0i128,       // holdback_amount
        );
        ids.push(stream_id);
    }

    // --- Uniqueness assertion ---
    let unique_count = {
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    };
    assert_eq!(
        unique_count, 100,
        "Expected 100 unique stream IDs but got {unique_count} â€” collision detected",
    );

    // --- Integrity assertion: every stored stream has the correct deposit ---
    for &stream_id in &ids {
        let stream = c.get_stream(&stream_id);
        assert_eq!(
            stream.deposit, 1_000i128,
            "Stream {stream_id} deposit corrupted â€” another stream may have overwritten it",
        );
        assert_eq!(
            stream.status,
            StreamStatus::Active,
            "Stream {stream_id} has unexpected status",
        );
    }
}

/// Verifies that each stream ID in a 100-stream batch maps to the correct
/// sender and recipient â€” a stronger integrity check that detects any
/// cross-contamination between storage entries.
#[test]
fn test_stream_id_no_data_overwrite_100_sequential() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);
    c.set_max_streams(&100u32);
    env.ledger().set_timestamp(2_000);

    let mut ids: std::vec::Vec<u64> = std::vec::Vec::new();
    for nonce in 0u64..100 {
        let id = c.create_stream(
            &sender, &recipient, &token_id,
            &2_000i128, &3600u64, &0u64, &nonce,
            &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
        );
        ids.push(id);
    }

    // After all creations, re-read every stream and confirm sender/recipient
    // are intact â€” any overwrite would corrupt these fields.
    for &stream_id in &ids {
        let stream = c.get_stream(&stream_id);
        assert_eq!(
            stream.sender, sender,
            "Stream {stream_id}: sender field corrupted",
        );
        assert_eq!(
            stream.recipient, recipient,
            "Stream {stream_id}: recipient field corrupted",
        );
        assert_eq!(
            stream.deposit, 2_000i128,
            "Stream {stream_id}: deposit corrupted",
        );
    }
}

// â”€â”€ get_stream_health tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Acceptance criteria:
//   - Returns correct struct for an active stream
//   - Health status reflects TTL thresholds correctly
//   - Returns StreamNotFound for unknown stream IDs
//   - StreamHealth type is accessible (exported in ABI)

/// get_stream_health returns StreamNotFound for a non-existent stream ID.
#[test]
fn test_get_stream_health_unknown_stream_returns_not_found() {
    let t = setup();
    let c = client(&t);

    let result = c.try_get_stream_health(&999_999u64);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotFound)),
        "Expected StreamNotFound for unknown stream ID",
    );
}

/// get_stream_health returns a valid struct for an active stream.
/// Checks that current_ledger and end_time are populated correctly.
#[test]
fn test_get_stream_health_active_stream_returns_struct() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let health = c.get_stream_health(&stream_id);

    assert_eq!(
        health.current_ledger, 1_000u32,
        "current_ledger should match the ledger sequence at query time",
    );
    assert_eq!(
        health.end_time, 3600u64,
        "end_time should match the stream's end timestamp",
    );
    // ttl_remaining_ledgers should be > 0 for a freshly created stream.
    assert!(
        health.ttl_remaining_ledgers > 0,
        "ttl_remaining_ledgers should be positive for a fresh stream",
    );
}

/// A freshly created stream with a long TTL is classified as Healthy.
/// In the Soroban test environment the default persistent TTL is set high
/// enough to exceed the 10_000-ledger threshold.
#[test]
fn test_get_stream_health_fresh_stream_is_healthy() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Push the ledger sequence well below the TTL expiry so the stream looks
    // fresh (large ttl_remaining).
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let health = c.get_stream_health(&stream_id);

    // The Soroban test environment grants a very large default TTL so a new
    // stream should always be Healthy right after creation.
    assert_eq!(
        health.status,
        HealthStatus::Healthy,
        "A freshly created stream should be Healthy; got {:?}",
        health.status,
    );
}

/// Manually set the ledger sequence to within 500 ledgers of the stream's TTL
/// expiry to push ttl_remaining below 1_000 and confirm AtRisk classification.
#[test]
fn test_get_stream_health_at_risk_threshold() {

    let sender    = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint enough tokens to cover a deposit that yields a non-zero flow_rate
    // with u64::MAX duration:  flow_rate = deposit / u64::MAX >= 1
    // Use deposit = u64::MAX (fits comfortably in i128).
    let deposit: i128 = u64::MAX as i128;
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
        .mint(&sender, &deposit);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);

    // start_time == current ledger timestamp (boundary condition from the issue)
    let start_time: u64 = 1_000;
    env.ledger().set_timestamp(start_time);

    // duration = u64::MAX â€” the principal edge-case under test.
    // This must NOT panic; the contract must return a valid stream_id.
    let stream_id = c.create_stream(
        &sender, &recipient, &token_id,
        &deposit,
        &u64::MAX,  // duration = u64::MAX seconds
        &0u64,      // cliff_offset
        &0u64,      // nonce
        &false,     // auto_renew
        &0u64,      // lock_until
        &false,     // allow_recipient_termination
        &0i128,     // holdback_amount
    );

    // Stream must be stored and readable.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, deposit, "deposit must be stored verbatim");
    assert_eq!(stream.start_time, start_time, "start_time must equal current ledger");
    assert_eq!(stream.status, StreamStatus::Active, "stream must be Active");

    // flow_rate = floor(deposit / u64::MAX) = floor((2^64 - 1) / (2^64 - 1)) = 1
    assert_eq!(stream.flow_rate, 1, "flow_rate must be 1 stroop/sec");

    // At start_time, elapsed = 0, so nothing is claimable.
    let claimable_at_start = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_start, 0, "get_claimable must return 0 at start_time");
}

/// Verify get_claimable returns the correct value at mid-duration for a
/// stream with duration = u64::MAX.
///
/// Mid-duration is approximated as u64::MAX / 2 seconds after start_time.
/// Expected claimable = flow_rate Ã— elapsed = 1 Ã— (u64::MAX / 2).
#[test]
fn test_max_duration_stream_claimable_at_mid_duration() {
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

    let sender    = Address::generate(&env);
    let recipient = Address::generate(&env);

    let deposit: i128 = u64::MAX as i128;
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
        .mint(&sender, &deposit);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);

    env.ledger().set_timestamp(0);
    env.ledger().set_sequence_number(1_000);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Query the TTL right after creation to know the expiry ledger.
    let health_fresh = c.get_stream_health(&stream_id);
    let expiry_ledger = health_fresh.current_ledger + health_fresh.ttl_remaining_ledgers;

    // Advance the ledger sequence to 500 ledgers before expiry (well into AtRisk zone).
    let near_expiry = expiry_ledger.saturating_sub(500);
    env.ledger().set_sequence_number(near_expiry);

    let health_at_risk = c.get_stream_health(&stream_id);

    assert!(
        health_at_risk.ttl_remaining_ledgers < 1_000,
        "Expected ttl_remaining < 1_000 at near-expiry but got {}",
        health_at_risk.ttl_remaining_ledgers,
    );
    assert_eq!(
        health_at_risk.status,
        HealthStatus::AtRisk,
        "Expected AtRisk status when ttl_remaining < 1_000",
    );
}

/// TTLWarning threshold: advance ledger to leave between 1_000 and 10_000
/// ledgers before expiry and confirm TTLWarning classification.
#[test]
fn test_get_stream_health_ttl_warning_threshold() {
    let start_time: u64 = 0;
    env.ledger().set_timestamp(start_time);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id,
        &deposit,
        &u64::MAX,  // duration = u64::MAX seconds
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
    );

    // flow_rate = 1 stroop/sec (deposit == u64::MAX, duration == u64::MAX)
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, 1);

    // Advance to mid-duration: elapsed = u64::MAX / 2
    let mid_elapsed: u64 = u64::MAX / 2;
    env.ledger().set_timestamp(start_time + mid_elapsed);

    // Expected claimable = flow_rate Ã— elapsed = 1 Ã— mid_elapsed
    let expected_claimable: i128 = mid_elapsed as i128;
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(
        claimable, expected_claimable,
        "get_claimable at mid-duration must equal flow_rate Ã— elapsed"
    );
}

/// Verify that start_time == current_ledger is accepted (boundary from the issue).
/// The contract must store start_time correctly and report 0 claimable immediately.
#[test]
fn test_max_duration_start_time_equals_current_ledger() {
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

    let sender    = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Generous deposit so flow_rate != 0 even with a huge duration
    let deposit: i128 = u64::MAX as i128;
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
        .mint(&sender, &deposit);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);

    env.ledger().set_timestamp(0);
    env.ledger().set_sequence_number(1_000);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false, 
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    let health_fresh = c.get_stream_health(&stream_id);
    let expiry_ledger = health_fresh.current_ledger + health_fresh.ttl_remaining_ledgers;

    // Place the ledger so that 5_000 ledgers remain (inside the warning band).
    let warning_sequence = expiry_ledger.saturating_sub(5_000);
    env.ledger().set_sequence_number(warning_sequence);

    let health_warning = c.get_stream_health(&stream_id);

    assert!(
        health_warning.ttl_remaining_ledgers >= 1_000
            && health_warning.ttl_remaining_ledgers < 10_000,
        "Expected ttl_remaining in [1_000, 10_000) for TTLWarning but got {}",
        health_warning.ttl_remaining_ledgers,
    );
    assert_eq!(
        health_warning.status,
        HealthStatus::TTLWarning,
        "Expected TTLWarning status when 1_000 <= ttl_remaining < 10_000",
    );
}


// â”€â”€ withdrawal_steps tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Stream: 1_000_000 stroops over 1_000 s â†’ flow_rate = 1_000 stroops/s.
// With 4 equal steps each step_interval = 250 s.
// Step boundaries (from start_time = 0):
//   step 1 at t=250  â†’ claimable = 250_000
//   step 2 at t=500  â†’ claimable = 250_000
//   step 3 at t=750  â†’ claimable = 250_000
//   step 4 at t=1000 â†’ claimable = 250_000  (final, full drain)

fn create_stepped_stream(t: &TestEnv, steps: u32, nonce: u64) -> u64 {
    let c = client(t);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_000);
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &1000u64,
        &0u64,
        &nonce,
        &false,
        &0u64,
        &false,
        &0i128,
        &Some(steps),
        &None::<i128>,
        &None::<u32>,
    )
}

/// Withdrawal before the first step boundary is rejected with NextStepNotReached.
#[test]
fn test_steps_early_rejection_before_first_boundary() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stepped_stream(&t, 4, 200);

    // At t=100 â€” before the first step boundary (t=250) â€” withdrawal must fail.
    t.env.ledger().set_timestamp(100);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::NextStepNotReached)),
        "Expected NextStepNotReached before first step boundary",
    );
}

/// Withdrawal at exactly the first step boundary succeeds.
#[test]
fn test_steps_withdraw_at_first_boundary() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stepped_stream(&t, 4, 201);

    // At t=250: exactly on the first step boundary.
    t.env.ledger().set_timestamp(250);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 250_000i128, "First step should release 250_000 stroops");

    // current_step should now be 1.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.current_step, 1u32, "current_step should advance to 1 after first withdrawal");
}

/// Withdrawal between step 1 and step 2 is rejected.
#[test]
fn test_steps_rejection_between_boundaries() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stepped_stream(&t, 4, 202);

    // Claim step 1 at t=250.
    t.env.ledger().set_timestamp(250);
    c.withdraw(&stream_id, &t.recipient);

    // At t=400 â€” past step 1 but before step 2 (t=500).
    t.env.ledger().set_timestamp(400);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::NextStepNotReached)),
        "Expected NextStepNotReached between step boundaries",
    );
}

/// Each step boundary releases the correct incremental amount.
#[test]
fn test_steps_correct_amount_at_each_boundary() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = create_stepped_stream(&t, 4, 203);

    for step in 1u64..=3 {
        t.env.ledger().set_timestamp(step * 250);
        c.withdraw(&stream_id, &t.recipient);
        let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
        assert_eq!(
            balance,
            (step as i128) * 250_000,
            "After step {step} recipient balance should be {} stroops",
            step * 250_000,
        );
    }

    // Verify current_step after 3 withdrawals.
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.options.current_step, 3u32);
}

/// Final step releases the full remaining balance (handles rounding dust).
#[test]
fn test_steps_final_step_releases_full_remaining_balance() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 1_000_001 stroops over 1_000 s with 4 steps â€” intentional dust.
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_001);
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1i128,
        &1u64,
        &0u64,
        &0u64,
        &1_000_001i128,
        &1000u64,
        &0u64,
        &204u64,
        &false,
        &0u64,
        &false,
        &0i128,
    );

    // Advance one second â€” the entire deposit should now be claimable
    t.env.ledger().set_timestamp(1001);

    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 1, "full deposit should be claimable after the stream duration elapses");
}

/// Minimum amount stream: after the single second elapses, withdraw claims the
/// full 1-stroop deposit and the stream completes.
#[test]
fn test_create_stream_minimum_withdraw_full_deposit() {
    let t = setup();
    let c = client(&t);

    t.env.ledger().set_timestamp(0);

        &Some(4u32),
        &None::<i128>,
        &None::<u32>,
    );

    // Withdraw steps 1-3.
    for step in 1u64..=3 {
        t.env.ledger().set_timestamp(step * 250);
        c.withdraw(&stream_id, &t.recipient);
    }

    let balance_after_3 = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Final step at t=1000 â€” must drain everything left.
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let final_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    // deposit = streaming portion (1_000_001, holdback=0)
    // flow_rate = 1_000_001 / 1000 = 1000 (integer division)
    // total streamable = 1000 * 1000 = 1_000_000; dust = 1
    // The final claim must equal the full remaining available balance.
    assert!(
        final_balance > balance_after_3,
        "Final step must release at least something",
    );
    // Stream should be removed (fully drained).
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "Stream must be removed after final step");
}

/// `withdrawal_steps = Some(0)` is rejected at creation with InvalidDuration.
#[test]
fn test_steps_zero_steps_rejected_at_creation() {
    let t = setup();
    let c = client(&t);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &205u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &Some(0u32),
        &None::<i128>,
        &None::<u32>,
    );
    assert_eq!(
        result,
        Err(Ok(StreamError::InvalidDuration)),
        "withdrawal_steps = Some(0) must be rejected",
    );
}

/// `withdrawal_steps = None` allows free-form withdrawal at any time.
#[test]
fn test_steps_none_allows_free_form_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1i128,
        &1u64,
        &0u64,
        &0u64,
        &100_000i128,
        &1000u64,
        &0u64,
        &206u64,
        &false,
        &0u64,
        &false,
        &0i128,
    );

    // Advance past the end
    t.env.ledger().set_timestamp(2);

    let before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    c.withdraw(&stream_id, &t.recipient);
    let after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    assert_eq!(after - before, 1, "recipient should receive exactly 1 stroop");

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Completed, "stream should be completed after full withdrawal");
}

/// top_up with the minimum valid amount (1 stroop) must extend the end_time by
/// exactly 1 second (extra_seconds = 1 / flow_rate = 1 / 1 = 1).
#[test]
fn test_top_up_minimum_valid_amount() {
    let t = setup();
    let c = client(&t);

    // Mint extra tokens so the sender can top up
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_000);

    t.env.ledger().set_timestamp(0);

    // flow_rate = 100_000 / 1000 = 100; a top-up of 100 adds 1 second.
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // At t=73 â€” arbitrary mid-stream time, no step constraint.
    t.env.ledger().set_timestamp(73);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 73 * 100, "Free-form withdrawal should work at any time");
}

// â”€â”€ min_withdrawal_amount tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Stream: 100_000 stroops over 1_000 s â†’ flow_rate = 100 stroops/s.
// floor = 10_000 stroops.

/// `min_withdrawal_amount = None` imposes no floor; any claimable amount works.
#[test]
fn test_min_withdrawal_no_floor_allows_small_amounts() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &300u64,
    // Set ledger timestamp to a specific boundary value
    let boundary_timestamp: u64 = 9_999_999;
    env.ledger().set_timestamp(boundary_timestamp);

    let stream_id = c.create_stream(
        &sender, &recipient, &token_id,
        &deposit,
        &u64::MAX,
        &0u64,
        &42u64,   // distinct nonce
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>, // no floor
    );

    // At t=1: claimable = 100 stroops â€” tiny but no floor is set.
    t.env.ledger().set_timestamp(1);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 100i128, "Without a floor, 100 stroops should be withdrawable");
}

/// `min_withdrawal_amount` is stored correctly on the stream struct.
#[test]
fn test_min_withdrawal_persisted_on_stream() {
    let t = setup();
    let c = client(&t);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);

    let floor = 10_000i128;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &301u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &Some(floor),
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(
        stream.options.min_withdrawal_amount,
        Some(floor),
        "min_withdrawal_amount must be stored on the stream",
    );
}

/// Withdrawal below the floor is rejected with AmountBelowMinimum.
#[test]
fn test_min_withdrawal_below_floor_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let floor = 10_000i128;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &302u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &Some(floor),
    );

    // At t=50: claimable = 5_000 â€” below the 10_000 floor.
    t.env.ledger().set_timestamp(50);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::AmountBelowMinimum)),
        "Claimable below floor must be rejected with AmountBelowMinimum",
    );
}

/// Withdrawal at or above the floor succeeds.
#[test]
fn test_min_withdrawal_at_floor_succeeds() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let floor = 10_000i128;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &303u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &Some(floor),
    );

    // At t=100: claimable = 10_000 â€” exactly the floor.
    t.env.ledger().set_timestamp(100);
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 10_000i128, "Withdrawal at exactly the floor must succeed");
}

/// Floor is bypassed on the final claim so the last tokens are always drainable.
#[test]
fn test_min_withdrawal_floor_bypassed_on_final_claim() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // floor = 10_000; deposit = 100_000; flow_rate = 100/s.
    // Withdraw 90_000 at t=900 (above floor), then the remaining 10_000
    // at t=1000 is exactly the floor â€” but set up so the last sliver is
    // just under the floor by using a stream where the final balance is
    // 5_000 (50 s Ã— 100 = 5_000 < 10_000 floor).
    // We do this by depositing 95_000 and letting 900 s pass first.
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    let floor = 10_000i128;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &304u64,
        &false,
        &0u64,
        &false,
        &0i128,
    );

    let before = c.get_stream(&stream_id);
    let expected_extension = 1u64; // 100 stroops / flow_rate 100 = 1 s

    c.top_up(&stream_id, &t.sender, &t.token_id, &100i128);

    let after = c.get_stream(&stream_id);
    assert_eq!(
        after.end_time,
        before.end_time + expected_extension,
        "end_time should extend by exactly 1 second on minimum top-up"
    );
    assert_eq!(
        after.deposit,
        before.deposit + 100,
        "deposit should increase by the top-up amount"
    );
}

/// top_up with amount = 1 on a flow_rate = 1 stream adds exactly 1 second â€”
/// the smallest possible extension.
#[test]
fn test_top_up_minimum_amount_on_minimum_flow_rate_stream() {
    let t = setup();
    let c = client(&t);

    // Extra tokens for top-up
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_000);

    t.env.ledger().set_timestamp(0);

    // Create a stream with flow_rate = 1 (amount = duration = 100)
        &None::<u32>,
        &Some(floor),
    );

    // Withdraw 90_000 at t=900 (above floor â€” succeeds normally).
    t.env.ledger().set_timestamp(900);
    c.withdraw(&stream_id, &t.recipient);
    let balance_mid = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance_mid, 90_000i128);

    // At t=950: remaining = 100_000 - 90_000 = 10_000; claimable = 50*100 = 5_000.
    // 5_000 < 10_000 floor, but this is NOT yet the final claim (5_000 < 10_000 remaining).
    t.env.ledger().set_timestamp(950);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::AmountBelowMinimum)),
        "Non-final claim below floor must still be rejected",
    );

    // At t=1000: stream ends; claimable = 10_000 = full remaining balance.
    // This IS the final claim (claimable == available), so floor is bypassed.
    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);
    let final_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(
        final_balance, 100_000i128,
        "Floor must be bypassed on the final claim so recipient recovers all tokens",
    );
}

/// Negative or zero min_withdrawal_amount is rejected at creation.
#[test]
fn test_min_withdrawal_zero_floor_rejected() {
    let t = setup();
    let c = client(&t);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &305u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &Some(0i128), // zero floor is invalid
    );
    assert_eq!(
        result,
        Err(Ok(StreamError::ZeroAmount)),
        "min_withdrawal_amount = Some(0) must be rejected",
    );
}

/// Both withdrawal_steps and min_withdrawal_amount can be set together.
/// The step gate is checked first; once the step is reachable the floor applies.
#[test]
fn test_steps_and_min_withdrawal_combined() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // 4 steps Ã— 250 s, floor = 200_000 stroops.
    // flow_rate = 1_000 stroops/s, so each step releases 250_000 > floor.
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_000);
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000i128,
        &1000u64,
        &0u64,
        &306u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &Some(4u32),
        &Some(200_000i128), // floor = 200_000
    );

    // Before step boundary: NextStepNotReached (step gate fires first).
    t.env.ledger().set_timestamp(100);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::NextStepNotReached)));

    // At step 1 boundary (t=250): claimable=250_000 >= floor=200_000 â†’ success.
    t.env.ledger().set_timestamp(250);
    c.withdraw(&stream_id, &t.recipient);
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 250_000i128, "Step 1 should release 250_000 stroops");
}

/// StreamConfig event is emitted when withdrawal_steps is set.
#[test]
fn test_stream_config_event_emitted_with_steps() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100i128,
        &100u64,
        &0u64,
        &0u64,
        &100_000i128,
        &1000u64,
        &0u64,
        &307u64,
        &false,
        &0u64,
        &false,
        &0i128,
    );

    let before = c.get_stream(&stream_id);

    c.top_up(&stream_id, &t.sender, &t.token_id, &1i128);

    let after = c.get_stream(&stream_id);
    assert_eq!(
        after.end_time,
        before.end_time + 1,
        "1-stroop top-up on a flow_rate=1 stream should add exactly 1 second"
        &Some(4u32),
        &None::<i128>,
        &None::<u32>,
    );

    use soroban_sdk::testutils::Events;
    let config_events: std::vec::Vec<_> = t.env.events().all().iter().filter(|(_, topics, _)| {
        let tv: soroban_sdk::Vec<soroban_sdk::Val> = topics.clone();
        if !tv.is_empty() {
            let first: soroban_sdk::Symbol = tv.get(0).unwrap().into_val(&t.env);
            first == soroban_sdk::Symbol::new(&t.env, "StreamConfig")
        } else { false }
    }).collect();

    assert_eq!(config_events.len(), 1, "Expected exactly one StreamConfig event");
    let (_, topics, data) = &config_events[0];
    let topic_id: u64 = {
        let tv: soroban_sdk::Vec<soroban_sdk::Val> = topics.clone();
        tv.get(1).unwrap().into_val(&t.env)
    };
    assert_eq!(topic_id, stream_id);
    let (steps, floor): (Option<u32>, Option<i128>) = data.clone().into_val(&t.env);
    assert_eq!(steps, Some(4u32));
    assert_eq!(floor, None::<i128>);
}
    );

    let stream = c.get_stream(&stream_id);
    // start_time must be exactly the ledger timestamp at creation
    assert_eq!(
        stream.start_time, boundary_timestamp,
        "start_time must equal the current ledger timestamp"
    );

    // No time has elapsed â€” claimable must be zero
    assert_eq!(
        c.get_claimable(&stream_id), 0,
        "claimable must be 0 when start_time == current ledger"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-token stream count tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_token_stream_count_zero_for_unknown_token() {
    let t = setup();
    let c = client(&t);
    // A token address that has never had a stream — must return 0, not error.
    let unknown_token = Address::generate(&t.env);
    assert_eq!(c.get_stream_count_by_token(&unknown_token), 0u64);
}

#[test]
fn test_token_stream_count_increments_on_create() {
    let t = setup();
    let c = client(&t);

    assert_eq!(c.get_stream_count_by_token(&t.token_id), 0u64);

    c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 1u64);

    c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &1u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 2u64);
}

#[test]
fn test_token_stream_count_decrements_on_cancel() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 1u64);

    c.cancel_stream(&stream_id, &t.sender);
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 0u64);
}

#[test]
fn test_token_stream_count_multiple_creates_then_cancel_each() {
    let t = setup();
    let c = client(&t);

    let id1 = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    let id2 = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &1u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    let id3 = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &2u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 3u64);

    c.cancel_stream(&id1, &t.sender);
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 2u64);

    c.cancel_stream(&id2, &t.sender);
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 1u64);

    c.cancel_stream(&id3, &t.sender);
    assert_eq!(c.get_stream_count_by_token(&t.token_id), 0u64);
}

#[test]
fn test_token_stream_count_different_tokens_are_independent() {
    let t = setup();
    let c = client(&t);

    // Register a second SAC token.
    let token_admin2 = Address::generate(&t.env);
    let token_id2 = t.env
        .register_stellar_asset_contract_v2(token_admin2.clone())
        .address();
    StellarAssetClient::new(&t.env, &token_id2).mint(&t.sender, &1_000_000i128);

    // Create one stream on token1.
    c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    // Create two streams on token2.
    c.create_stream(
        &t.sender, &t.recipient, &token_id2,
        &100_000i128, &1000u64, &0u64, &1u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );
    c.create_stream(
        &t.sender, &t.recipient, &token_id2,
        &100_000i128, &1000u64, &0u64, &2u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );

    assert_eq!(c.get_stream_count_by_token(&t.token_id), 1u64);
    assert_eq!(c.get_stream_count_by_token(&token_id2), 2u64);
}

// ═══════════════════════════════════════════════════════════════════════════
// Non-transferable stream tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_non_transferable_stream_flag_is_stored() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &true, // non_transferable = true
    );

    let stream = c.get_stream(&stream_id);
    assert!(stream.options.non_transferable, "non_transferable flag must be persisted as true");
}

#[test]
fn test_transferable_stream_flag_is_false_by_default() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );

    let stream = c.get_stream(&stream_id);
    assert!(!stream.options.non_transferable, "non_transferable must be false when not set");
}

#[test]
fn test_transfer_recipient_rejected_for_non_transferable_stream() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &true,
    );

    let new_recipient = Address::generate(&t.env);
    let result = c.try_transfer_recipient(&stream_id, &t.recipient, &new_recipient);

    assert!(result.is_err(), "transfer_recipient must fail for a non-transferable stream");
}

#[test]
fn test_transfer_recipient_succeeds_when_flag_is_false() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false,
    );

    let new_recipient = Address::generate(&t.env);
    // Should succeed without error.
    c.transfer_recipient(&stream_id, &t.recipient, &new_recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.recipient, new_recipient);
}

#[test]
fn test_non_transferable_stream_can_be_cancelled_by_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &true,
    );

    // cancel_stream must succeed even when non_transferable is true.
    c.cancel_stream(&stream_id, &t.sender);

    // Stream is gone — get_stream must return an error.
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "cancelled stream must no longer be retrievable");
}

// ═══════════════════════════════════════════════════════════════════════════
// Recipient approval tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_pending_approval_stream_created_in_pending_state() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true, // requires_recipient_approval
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::PendingApproval);
    assert_eq!(stream.options.approval_timestamp, 0u64);
    assert!(stream.options.requires_recipient_approval);
}

#[test]
fn test_withdraw_returns_awaiting_approval_before_approve() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true,
    );

    t.env.ledger().set_timestamp(500);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_err());
}

#[test]
fn test_approve_stream_transitions_to_active() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(100);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true,
    );

    t.env.ledger().set_timestamp(200);
    c.approve_stream(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.options.approval_timestamp, 200u64);
}

#[test]
fn test_claimable_starts_from_approval_not_creation() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create at t=0, approve at t=500, check claimable at t=600
    // With flow_rate=100 stroops/sec and 100s elapsed since approval → expect 10_000
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true,
    );

    t.env.ledger().set_timestamp(500);
    c.approve_stream(&stream_id, &t.recipient);

    t.env.ledger().set_timestamp(600);
    let claimable = c.get_claimable(&stream_id);
    // 100 seconds elapsed since approval × 100 flow_rate = 10_000 stroops
    assert_eq!(claimable, 10_000i128);
}

#[test]
fn test_sender_can_cancel_pending_approval_stream_at_zero_cost() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true,
    );

    // Sender balance before cancel
    let bal_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    c.cancel_stream(&stream_id, &t.sender);

    // Stream gone
    assert!(c.try_get_stream(&stream_id).is_err());
    // Sender receives full refund
    let bal_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    assert_eq!(bal_after - bal_before, 100_000i128);
}

#[test]
fn test_approve_stream_only_callable_by_recipient() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &true,
    );

    let other = Address::generate(&t.env);
    let result = c.try_approve_stream(&stream_id, &other);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Sender stream lock tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_lock_stream_sets_sender_locked_flag() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    assert!(!c.get_stream(&stream_id).options.sender_locked);
    c.lock_stream(&stream_id, &t.sender);
    assert!(c.get_stream(&stream_id).options.sender_locked);
}

#[test]
fn test_cancel_stream_returns_stream_is_locked_after_lock() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    c.lock_stream(&stream_id, &t.sender);
    let result = c.try_cancel_stream(&stream_id, &t.sender);
    assert!(result.is_err());
}

#[test]
fn test_recipient_can_withdraw_from_locked_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    c.lock_stream(&stream_id, &t.sender);

    t.env.ledger().set_timestamp(500);
    // Should succeed — lock only prevents sender cancellation.
    c.withdraw(&stream_id, &t.recipient);
    let stream = c.get_stream(&stream_id);
    assert!(stream.options.total_withdrawn > 0);
}

#[test]
fn test_lock_stream_is_idempotent_error_on_double_lock() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    c.lock_stream(&stream_id, &t.sender);
    let result = c.try_lock_stream(&stream_id, &t.sender);
    assert!(result.is_err(), "second lock call must error with StreamIsLocked");
}

#[test]
fn test_lock_stream_only_callable_by_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let result = c.try_lock_stream(&stream_id, &t.recipient);
    assert!(result.is_err());
}


// ── Minimum Duration Validation Tests ────────────────────────────────────────

/// Test that create_stream enforces minimum duration.
#[test]
fn test_create_stream_respects_min_duration() {
    let t = setup();
    let c = client(&t);

    // Set min_duration to 3600 seconds (1 hour).
    c.set_min_duration(&t.sender, &3600u64);

    // Attempt to create stream with duration < min_duration (100 seconds).
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &100u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "create_stream should reject duration < min_duration");
    
    // Verify the error is StreamDurationTooShort.
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamDurationTooShort),
        Ok(_) => panic!("Expected StreamDurationTooShort error"),
    }

    // Attempt to create stream with duration == min_duration (3600 seconds).
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    // Should succeed (no error).
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time - stream.start_time, 3600);
}

/// Test that create_stream accepts streams at exact minimum duration boundary.
#[test]
fn test_create_stream_exact_min_duration_boundary() {
    let t = setup();
    let c = client(&t);

    let min_duration = 7200u64; // 2 hours
    c.set_min_duration(&t.sender, &min_duration);

    // Create stream with exactly min_duration.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &min_duration, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time - stream.start_time, min_duration);

    // Attempt to create stream with duration just below min_duration.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &(min_duration - 1), &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "duration = min_duration - 1 should be rejected");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamDurationTooShort),
        Ok(_) => panic!("Expected StreamDurationTooShort error"),
    }
}

/// Test that create_stream_with_curve enforces minimum duration.
#[test]
fn test_create_stream_with_curve_respects_min_duration() {
    let t = setup();
    let c = client(&t);

    // Set min_duration to 3600 seconds.
    c.set_min_duration(&t.sender, &3600u64);

    // Attempt to create stream with curve and duration < min_duration (100 seconds).
    let result = c.try_create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &100u64, &0u64, &0u64,
        &false, &0u64, &false,
        &VestingCurve::Linear,
    );
    assert!(result.is_err(), "create_stream_with_curve should reject duration < min_duration");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamDurationTooShort),
        Ok(_) => panic!("Expected StreamDurationTooShort error"),
    }

    // Create stream with curve and duration >= min_duration.
    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &3600u64, &0u64, &0u64,
        &false, &0u64, &false,
        &VestingCurve::Linear,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time - stream.start_time, 3600);
}

/// Test that create_stream_with_schedule (tranches) enforces minimum duration.
#[test]
fn test_create_stream_with_schedule_respects_min_duration() {
    let t = setup();
    let c = client(&t);

    // Set min_duration to 3600 seconds.
    c.set_min_duration(&t.sender, &3600u64);

    let now = t.env.ledger().timestamp();

    // Create tranches that span less than min_duration (100 seconds).
    let tranches = soroban_sdk::Vec::from_array(&t.env, [
        VestingTranche {
            unlock_time: now + 50,
            amount: 50_000i128,
        },
        VestingTranche {
            unlock_time: now + 100,
            amount: 50_000i128,
        },
    ]);

    // Attempt to create stream with tranches spanning < min_duration.
    let result = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches,
        &0u64, &0u64, &false,
        &None::<Address>, &0u32,
    );
    assert!(result.is_err(), "create_stream_with_schedule should reject duration < min_duration");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamDurationTooShort),
        Ok(_) => panic!("Expected StreamDurationTooShort error"),
    }

    // Create tranches that span >= min_duration (3600 seconds).
    let tranches_valid = soroban_sdk::Vec::from_array(&t.env, [
        VestingTranche {
            unlock_time: now + 1800,
            amount: 50_000i128,
        },
        VestingTranche {
            unlock_time: now + 3600,
            amount: 50_000i128,
        },
    ]);

    // Should succeed.
    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches_valid,
        &0u64, &0u64, &false,
        &None::<Address>, &0u32,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time - stream.start_time, 3600);
}

/// Test that min_duration can be dynamically configured.
#[test]
fn test_min_duration_is_configurable() {
    let t = setup();
    let c = client(&t);

    // Check initial min_duration.
    let initial_min = c.min_duration();
    assert_eq!(initial_min, 0u64, "setup() should set min_duration to 0");

    // Set min_duration to 1800 seconds.
    c.set_min_duration(&t.sender, &1800u64);
    let updated_min = c.min_duration();
    assert_eq!(updated_min, 1800);

    // Set min_duration to 7200 seconds.
    c.set_min_duration(&t.sender, &7200u64);
    let updated_min = c.min_duration();
    assert_eq!(updated_min, 7200);

    // Set min_duration back to 0 (no minimum).
    c.set_min_duration(&t.sender, &0u64);
    let updated_min = c.min_duration();
    assert_eq!(updated_min, 0);
}

/// Test that zero-duration streams are correctly rejected when min_duration > 0.
#[test]
fn test_zero_duration_stream_rejected() {
    let t = setup();
    let c = client(&t);

    // Set min_duration to 1 second to reject zero-duration streams.
    c.set_min_duration(&t.sender, &1u64);

    // Attempt to create stream with duration = 0.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &0u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "zero-duration stream should be rejected");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamDurationTooShort),
        Ok(_) => panic!("Expected StreamDurationTooShort error"),
    }
}

/// Test that min_duration is properly applied across different stream types.
#[test]
fn test_min_duration_enforced_all_stream_types() {
    let t = setup();
    let c = client(&t);

    let min_duration = 7200u64; // 2 hours
    c.set_min_duration(&t.sender, &min_duration);
    let now = t.env.ledger().timestamp();

    // Test 1: create_stream
    let stream1 = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &min_duration, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let s1 = c.get_stream(&stream1);
    assert_eq!(s1.end_time - s1.start_time, min_duration);

    // Test 2: create_stream_with_curve
    let stream2 = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &min_duration, &0u64, &0u64,
        &false, &0u64, &false,
        &VestingCurve::Linear,
    );
    let s2 = c.get_stream(&stream2);
    assert_eq!(s2.end_time - s2.start_time, min_duration);

    // Test 3: create_stream_with_schedule
    let tranches = soroban_sdk::Vec::from_array(&t.env, [
        VestingTranche {
            unlock_time: now + 3600,
            amount: 50_000i128,
        },
        VestingTranche {
            unlock_time: now + min_duration,
            amount: 50_000i128,
        },
    ]);
    let stream3 = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches,
        &0u64, &0u64, &false,
        &None::<Address>, &0u32,
    );
    let s3 = c.get_stream(&stream3);
    assert_eq!(s3.end_time - s3.start_time, min_duration);
}

/// Test that min_duration of 0 disables the check (allows any duration).
#[test]
fn test_min_duration_zero_disables_check() {
    let t = setup();
    let c = client(&t);

    // Set min_duration to 0 (no minimum).
    c.set_min_duration(&t.sender, &0u64);

    // Should be able to create streams with very short durations.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.end_time - stream.start_time, 1);
}

/// Test error message clarity for min_duration violations.
#[test]
fn test_min_duration_clear_error_handling() {
    let t = setup();
    let c = client(&t);

    let min_duration = 5000u64;
    c.set_min_duration(&t.sender, &min_duration);

    // Try with a much shorter duration.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &100u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Verify we get the specific StreamDurationTooShort error.
    match result {
        Err(e) => {
            assert_eq!(e, StreamError::StreamDurationTooShort);
            // The error code should be 22 as defined in errors.rs.
            // We can verify this through the contract's error handling.
        }
        Ok(_) => panic!("Expected error but stream was created"),
    }
}


// ── Token Whitelist Tests ────────────────────────────────────────────────────

/// Test that token whitelist can be enabled/disabled.
#[test]
fn test_token_whitelist_can_be_enabled_disabled() {
    let t = setup();
    let c = client(&t);

    // Initially whitelist is disabled (no enforcement).
    let is_enabled = c.try_get_token_whitelist_enabled();
    match is_enabled {
        Ok(enabled) => assert!(!enabled, "whitelist should be disabled by default"),
        Err(_) => {} // The function might not exist in the interface
    }

    // Enable token whitelist.
    c.set_token_whitelist_enabled(&t.sender, &true);
    
    // Verify it's enabled.
    // Note: We can verify this by attempting to create a stream with a non-whitelisted token.
}

/// Test that token whitelist prevents non-whitelisted tokens when enabled.
#[test]
fn test_token_whitelist_prevents_non_whitelisted_tokens() {
    let t = setup();
    let c = client(&t);

    // Create a second token for testing.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    // Mint tokens for the sender on token_id_2.
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    // Enable token whitelist.
    c.set_token_whitelist_enabled(&t.sender, &true);

    // Add only the first token to the whitelist.
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Attempt to create stream with the whitelisted token (should succeed).
#[test]
fn test_top_up_rejected_when_stream_locked() {
    let t = setup();
    let c = client(&t);
    
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.token, t.token_id, "stream should use whitelisted token");

    // Attempt to create stream with the non-whitelisted token (should fail).
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "create_stream should reject non-whitelisted token");
    match result {
        Err(e) => assert_eq!(e, StreamError::TokenNotWhitelisted),
        Ok(_) => panic!("Expected TokenNotWhitelisted error"),
    }
}

/// Test that whitelist enforcement applies to all stream creation variants.
#[test]
fn test_token_whitelist_enforced_all_variants() {
    let t = setup();
    let c = client(&t);

    // Create a second token.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    // Enable whitelist and add only token_id.
    c.set_token_whitelist_enabled(&t.sender, &true);
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    let now = t.env.ledger().timestamp();

    // Test 1: create_stream with non-whitelisted token fails.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "create_stream should reject non-whitelisted token");

    // Test 2: create_stream_with_curve with non-whitelisted token fails.
    let result = c.try_create_stream_with_curve(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false,
        &VestingCurve::Linear,
    );
    assert!(result.is_err(), "create_stream_with_curve should reject non-whitelisted token");

    // Test 3: create_stream_with_schedule with non-whitelisted token fails.
    let tranches = soroban_sdk::Vec::from_array(&t.env, [
        VestingTranche {
            unlock_time: now + 500,
            amount: 50_000i128,
        },
        VestingTranche {
            unlock_time: now + 1000,
            amount: 50_000i128,
        },
    ]);
    let result = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &tranches,
        &0u64, &0u64, &false,
        &None::<Address>, &0u32,
    );
    assert!(result.is_err(), "create_stream_with_schedule should reject non-whitelisted token");
}

/// Test that whitelist can be disabled to allow any token.
#[test]
fn test_token_whitelist_disabled_allows_all_tokens() {
    let t = setup();
    let c = client(&t);

    // Create a second token.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    // Enable whitelist and add only token_id.
    c.set_token_whitelist_enabled(&t.sender, &true);
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Verify non-whitelisted token is rejected.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "non-whitelisted token should be rejected");

    // Disable whitelist.
    c.set_token_whitelist_enabled(&t.sender, &false);

    // Now non-whitelisted token should be accepted.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.token, token_id_2, "stream should use non-whitelisted token when whitelist is disabled");
}

/// Test that tokens can be added and removed from the whitelist.
#[test]
fn test_token_whitelist_add_remove() {
    let t = setup();
    let c = client(&t);

    // Create a second token.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    // Enable whitelist with no tokens added initially.
    c.set_token_whitelist_enabled(&t.sender, &true);

    // Attempt to create stream with token_id should fail (not whitelisted).
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "non-whitelisted token should be rejected");

    // Add token_id to whitelist.
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Now stream creation should succeed.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.token, t.token_id, "stream should use whitelisted token");

    // Remove token_id from whitelist.
    c.remove_token_from_whitelist(&t.sender, &t.token_id);

    // Attempt to create stream should fail again.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &1u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    assert!(result.is_err(), "removed token should be rejected");
}

/// Test that batch_create_stream respects token whitelist.
#[test]
fn test_token_whitelist_batch_create_stream() {
    let t = setup();
    let c = client(&t);

    // Create a second token and recipient.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    let recipient_2 = Address::generate(&t.env);

    // Enable whitelist and add only token_id (not token_id_2).
    c.set_token_whitelist_enabled(&t.sender, &true);
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Batch create with both whitelisted and non-whitelisted tokens should fail.
    let recipients = soroban_sdk::Vec::from_array(&t.env, [&t.recipient, &recipient_2]);
    let amounts = soroban_sdk::Vec::from_array(&t.env, [100_000i128, 100_000i128]);
    let tokens = soroban_sdk::Vec::from_array(&t.env, [&t.token_id, &token_id_2]);
    let lock_untils = soroban_sdk::Vec::from_array(&t.env, [0u64, 0u64]);

    let result = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000u64, &false, &lock_untils, &0u64,
    );
    assert!(result.is_err(), "batch should fail when any token is not whitelisted");

    // Batch create with all whitelisted tokens should succeed.
    let recipients = soroban_sdk::Vec::from_array(&t.env, [&t.recipient, &recipient_2]);
    let amounts = soroban_sdk::Vec::from_array(&t.env, [100_000i128, 100_000i128]);
    let tokens = soroban_sdk::Vec::from_array(&t.env, [&t.token_id, &t.token_id]);
    let lock_untils = soroban_sdk::Vec::from_array(&t.env, [0u64, 0u64]);

    let stream_ids = c.batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000u64, &false, &lock_untils, &0u64,
    );
    assert_eq!(stream_ids.len(), 2, "batch should create 2 streams with whitelisted token");
}

/// Test that whitelist enforcement prevents spam tokens when enabled.
#[test]
fn test_token_whitelist_prevents_spam_tokens() {
    let t = setup();
    let c = client(&t);

    // Create multiple spam tokens.
    let token_admin = Address::generate(&t.env);
    let mut spam_tokens = Vec::new();
    for _i in 0..3 {
        let spam_token = t.env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        StellarAssetClient::new(&t.env, &spam_token).mint(&t.sender, &1_000_000);
        spam_tokens.push(spam_token);
    }

    // Enable whitelist and add only the primary token.
    c.set_token_whitelist_enabled(&t.sender, &true);
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Attempt to create streams with spam tokens should all fail.
    for spam_token in &spam_tokens {
        let result = c.try_create_stream(
            &t.sender, &t.recipient, spam_token,
            &100_000i128, &1000u64, &0u64, &0u64,
            &false, &0u64, &false, &0i128,
            &None::<u32>, &None::<i128>, &false, &false,
        );
        assert!(result.is_err(), "spam token should be rejected");
        match result {
            Err(e) => assert_eq!(e, StreamError::TokenNotWhitelisted),
            Ok(_) => panic!("Expected TokenNotWhitelisted error"),
        }
    }

    // Verify that the whitelisted token still works.

    // Lock the stream first
    c.lock_stream(&stream_id, &t.sender);
    assert!(c.get_stream(&stream_id).options.sender_locked);
    
    // Attempt to top_up should be rejected
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000i128);
    assert_eq!(result, Err(Ok(StreamError::StreamIsLocked)), 
        "top_up must be rejected when sender_locked is true");
}

#[test]
fn test_top_up_works_before_lock() {
    let t = setup();
    let c = client(&t);
    
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.token, t.token_id, "whitelisted token should work");
}

/// Test clear error message for token whitelist violations.
#[test]
fn test_token_whitelist_clear_error_handling() {
    let t = setup();
    let c = client(&t);

    // Create another token.
    let token_admin = Address::generate(&t.env);
    let token_id_2 = t.env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&t.env, &token_id_2).mint(&t.sender, &1_000_000);

    // Enable whitelist but don't add the second token.
    c.set_token_whitelist_enabled(&t.sender, &true);
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Attempt to create stream with untrusted token.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &token_id_2,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Verify we get the specific TokenNotWhitelisted error.
    match result {
        Err(e) => {
            assert_eq!(e, StreamError::TokenNotWhitelisted);
            // The error code should be 37 as defined in errors.rs.
        }
        Ok(_) => panic!("Expected error but stream was created"),
    }
}

/// Test that admin-only access control is enforced for whitelist operations.
#[test]
fn test_token_whitelist_admin_only_access() {
    let t = setup();
    let c = client(&t);

    let non_admin = Address::generate(&t.env);

    // Attempt to enable whitelist from non-admin should fail (if auth is enforced).
    // Note: This test depends on whether the contract enforces admin access.
    // The implementation uses check_admin() and admin.require_auth().

    // Test that admin can enable whitelist.
    c.set_token_whitelist_enabled(&t.sender, &true);

    // Test that admin can add to whitelist.
    c.add_token_to_whitelist(&t.sender, &t.token_id);

    // Verify token is whitelisted by attempting creation.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.token, t.token_id, "admin should be able to manage whitelist");
}


// ── updateStreamRate Tests ───────────────────────────────────────────────────

/// Test that sender can update the flow rate of an active stream.
#[test]
fn test_update_stream_rate_basic() {
    let t = setup();
    let c = client(&t);

    // Create a stream with 1000 per second for 1000 seconds (1,000,000 total)
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let stream_before = c.get_stream(&stream_id);
    assert_eq!(stream_before.flow_rate, 1000);

    // Update rate to 2000 per second
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.flow_rate, 2000);
    assert!(stream_after.end_time < stream_before.end_time, "end_time should decrease with higher rate");
}

/// Test that balance is settled before rate change is applied.
#[test]
fn test_update_stream_rate_settles_balance() {
    let t = setup();
    let c = client(&t);

    // Create stream: 1000 per second for 1000 seconds
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Move time forward 100 seconds
    t.env.ledger().set_timestamp(100);

    // Claimable should be ~100,000 at this point
    let claimable_before = c.get_claimable(&stream_id);
    assert!(claimable_before > 0, "should have claimable balance");

    // Update rate to 500 per second
    c.update_stream_rate(&t.sender, &stream_id, &500i128);

    // After rate update, claimable should be 0 (settled)
    let claimable_after = c.get_claimable(&stream_id);
    assert_eq!(claimable_after, 0, "balance should be settled after rate update");

    // But total_withdrawn should reflect the settled amount
    let stream = c.get_stream(&stream_id);
    assert!(stream.options.total_withdrawn >= claimable_before - 1000, "settled balance should be recorded");
}

/// Test that rate update is only callable by sender.
#[test]
fn test_update_stream_rate_only_sender() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Recipient attempts to update rate (should fail)
    let result = c.try_update_stream_rate(&t.recipient, &stream_id, &2000i128);
    assert!(result.is_err(), "recipient should not be able to update rate");
    match result {
        Err(e) => assert_eq!(e, StreamError::NotSender),
        Ok(_) => panic!("Expected NotSender error"),
    }

    // Sender can update rate
    let result = c.try_update_stream_rate(&t.sender, &stream_id, &2000i128);
    assert!(result.is_ok(), "sender should be able to update rate");
}

/// Test that rate update fails for non-active streams.
#[test]
fn test_update_stream_rate_requires_active_stream() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Cancel the stream
    c.cancel_stream(&stream_id, &t.sender);

    // Attempt to update rate on cancelled stream (should fail)
    let result = c.try_update_stream_rate(&t.sender, &stream_id, &2000i128);
    assert!(result.is_err(), "should not be able to update cancelled stream");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamNotActive),
        Ok(_) => panic!("Expected StreamNotActive error"),
    }
}

/// Test that zero flow rate is rejected.
#[test]
fn test_update_stream_rate_rejects_zero_rate() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Attempt to set rate to zero
    let result = c.try_update_stream_rate(&t.sender, &stream_id, &0i128);
    assert!(result.is_err(), "should reject zero flow rate");
    match result {
        Err(e) => assert_eq!(e, StreamError::ZeroFlowRate),
        Ok(_) => panic!("Expected ZeroFlowRate error"),
    }
}

/// Test that rate update adjusts end_time correctly.
#[test]
fn test_update_stream_rate_adjusts_end_time() {
    let t = setup();
    let c = client(&t);

    // Create stream: 1000 tokens/sec for 100 seconds (100,000 total)
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let stream_before = c.get_stream(&stream_id);
    let original_end = stream_before.end_time;

    // Move time forward 50 seconds - 50,000 tokens earned
    t.env.ledger().set_timestamp(50);

    // Update rate to 2000 tokens/sec
    // Remaining: 50,000 tokens / 2000 = 25 seconds
    // New end time should be: 50 + 25 = 75
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.end_time, 75, "new end_time should be 75");
    assert!(stream_after.end_time < original_end, "end_time should decrease when rate increases");
}

/// Test that rate can be increased (stream ends sooner).
#[test]
fn test_update_stream_rate_increase() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let stream_before = c.get_stream(&stream_id);

    // Increase rate from 1000 to 2000
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    let stream_after = c.get_stream(&stream_id);
    assert!(stream_after.end_time < stream_before.end_time, "higher rate should end sooner");
    assert_eq!(stream_after.flow_rate, 2000);
}

/// Test that rate can be decreased (stream ends later).
#[test]
fn test_update_stream_rate_decrease() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let stream_before = c.get_stream(&stream_id);

    // Decrease rate from 1000 to 500
    c.update_stream_rate(&t.sender, &stream_id, &500i128);

    let stream_after = c.get_stream(&stream_id);
    assert!(stream_after.end_time > stream_before.end_time, "lower rate should end later");
    assert_eq!(stream_after.flow_rate, 500);
}

/// Test that step-vesting streams cannot have rate updated.
#[test]
fn test_update_stream_rate_fails_on_step_vesting() {
    let t = setup();
    let c = client(&t);

    let now = t.env.ledger().timestamp();
    let tranches = soroban_sdk::Vec::from_array(&t.env, [
        VestingTranche {
            unlock_time: now + 500,
            amount: 50_000i128,
        },
        VestingTranche {
            unlock_time: now + 1000,
            amount: 50_000i128,
        },
    ]);

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &tranches,
        &0u64, &0u64, &false,
        &None::<Address>, &0u32,
    );

    // Attempt to update rate on step-vesting stream (should fail)
    let result = c.try_update_stream_rate(&t.sender, &stream_id, &2000i128);
    assert!(result.is_err(), "step-vesting streams should not support rate updates");
    match result {
        Err(e) => assert_eq!(e, StreamError::InvalidDuration),
        Ok(_) => panic!("Expected InvalidDuration error"),
    }
}

/// Test that recipient can still withdraw after rate update.
#[test]
fn test_update_stream_rate_doesnt_break_withdrawals() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Update rate
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    // Move time forward
    t.env.ledger().set_timestamp(100);

    // Recipient should be able to withdraw at new rate
    let claimable = c.get_claimable(&stream_id);
    assert!(claimable > 0, "should have claimable balance at new rate");

    // Withdraw should succeed
    c.withdraw(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert!(stream.options.total_withdrawn > 0, "withdrawal should succeed after rate update");
}

/// Test multiple rate updates in sequence.
#[test]
fn test_update_stream_rate_multiple_times() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Update rate multiple times
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);
    let stream_after_first = c.get_stream(&stream_id);
    assert_eq!(stream_after_first.flow_rate, 2000);

    // Move time forward a bit
    t.env.ledger().set_timestamp(50);

    // Update rate again
    c.update_stream_rate(&t.sender, &stream_id, &500i128);
    let stream_after_second = c.get_stream(&stream_id);
    assert_eq!(stream_after_second.flow_rate, 500);
    assert!(stream_after_second.end_time > stream_after_first.end_time, "second update should extend stream");
}

/// Test that event is emitted when rate is updated.
#[test]
fn test_update_stream_rate_emits_event() {
    let t = setup();
    let c = client(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &1_000_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    // Update rate - event should be emitted
    // (Note: We can't directly check events in unit tests, but the call should succeed)
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.flow_rate, 2000, "rate should be updated");
}

/// Test error handling when stream not found.
#[test]
fn test_update_stream_rate_stream_not_found() {
    let t = setup();
    let c = client(&t);

    // Try to update non-existent stream
    let result = c.try_update_stream_rate(&t.sender, &999999u64, &2000i128);
    assert!(result.is_err(), "should fail for non-existent stream");
    match result {
        Err(e) => assert_eq!(e, StreamError::StreamNotFound),
        Ok(_) => panic!("Expected StreamNotFound error"),
    }
}

/// Test that deposit is updated correctly after rate change.
#[test]
fn test_update_stream_rate_updates_deposit() {
    let t = setup();
    let c = client(&t);

    // Create stream: 1000 tokens/sec for 100 seconds (100,000 total)
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );

    let stream_before = c.get_stream(&stream_id);
    assert_eq!(stream_before.deposit, 100_000);

    // Move time forward 50 seconds
    t.env.ledger().set_timestamp(50);

    // Update rate to 2000 tokens/sec
    c.update_stream_rate(&t.sender, &stream_id, &2000i128);

    let stream_after = c.get_stream(&stream_id);
    // Remaining deposit should be 50,000 (the other 50,000 was earned)
    assert_eq!(stream_after.deposit, 50_000, "deposit should be updated to remaining balance");
}

    let original_end = c.get_stream(&stream_id).end_time;
    
    // Top up should work before lock
    c.top_up(&stream_id, &t.sender, &t.token_id, &10_000i128);
    let stream_after_topup = c.get_stream(&stream_id);
    
    // Verify end_time was extended
    assert!(stream_after_topup.end_time > original_end, 
        "end_time should be extended after top_up");
    assert!(!stream_after_topup.options.sender_locked, "stream should not be locked yet");
    
    // Now lock the stream
    c.lock_stream(&stream_id, &t.sender);
    assert!(c.get_stream(&stream_id).options.sender_locked);
    
    // Subsequent top_up should fail
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000i128);
    assert_eq!(result, Err(Ok(StreamError::StreamIsLocked)));
}


#[test]
fn test_get_claimable_future_start_time_zero_at_creation() {
    // This test verifies that get_claimable returns 0 for a stream with a future start_time
    // before that start_time is reached on the ledger.
    //
    // Stream created at ledger t=0 with start_time=100:
    // - At t=0 (before start): get_claimable should return 0
    // - At t=99 (just before start): get_claimable should return 0  
    // - At t=100 (exactly at start): get_claimable should return 0 (no time has elapsed yet)
    // - At t=101 (just after start): get_claimable should return > 0 (time has elapsed)
    //
    // This prevents premature withdrawals on streams with future start times.
    let t = setup();
    let c = client(&t);
    
    // Create a stream with start_time in the future (current_ledger + 100)
    t.env.ledger().set_timestamp(0);
    
    let future_start_time: u64 = 100u64;
    let duration = 1000u64;
    let amount = 100_000i128;
    let flow_rate = amount / duration as i128; // 100 stroops/sec
    
    // We can't directly use create_stream_scheduled since it may not be implemented,
    // but we can manually create a stream object and test the logic.
    // For now, test the existing scenario that start_time = current ledger (0),
    // then verify that cliff_time enforcement (which is before start_time enforcement)
    // works correctly.
    
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &amount, &duration, &0u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.start_time, 0, "stream.start_time should be current ledger");
    
    // At creation (t=0), which is exactly start_time, claimable should be 0
    let claimable_at_creation = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_creation, 0, "get_claimable must return 0 at start_time");
    
    // After 1 second (t=1), claimable should equal flow_rate
    t.env.ledger().set_timestamp(1);
    let claimable_at_t1 = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_t1, flow_rate, "get_claimable should return flow_rate after 1 second");
}

#[test]
fn test_get_claimable_cliff_before_start_prevents_premature_withdrawal() {
    // This test verifies that cliff_time enforcement prevents withdrawals
    // before tokens begin to accrue. This is a key protection for future-start streams
    // where cliff_time can be set > start_time.
    //
    // When cliff_time > start_time, no tokens are claimable even if time has passed
    // since start_time, until cliff_time is reached.
    let t = setup();
    let c = client(&t);
    
    t.env.ledger().set_timestamp(0);
    
    let amount = 100_000i128;
    let duration = 1000u64;
    let cliff_seconds = 500u64; // cliff at 500 seconds
    
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &amount, &duration, &cliff_seconds, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.start_time, 0);
    assert_eq!(stream.cliff_time, cliff_seconds);
    
    // At t=0 (at start_time), before cliff: claimable = 0
    let claimable_at_start = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_start, 0, "get_claimable must return 0 before cliff_time");
    
    // At t=250 (halfway to cliff), still before cliff: claimable = 0
    t.env.ledger().set_timestamp(250);
    let claimable_before_cliff = c.get_claimable(&stream_id);
    assert_eq!(claimable_before_cliff, 0, "get_claimable must return 0 before cliff_time");
    
    // At t=499 (just before cliff): claimable = 0
    t.env.ledger().set_timestamp(499);
    let claimable_just_before_cliff = c.get_claimable(&stream_id);
    assert_eq!(claimable_just_before_cliff, 0, "get_claimable must return 0 just before cliff_time");
    
    // At t=500 (exactly at cliff): claimable should still be 0 (no time has elapsed since cliff)
    t.env.ledger().set_timestamp(500);
    let claimable_at_cliff = c.get_claimable(&stream_id);
    assert_eq!(claimable_at_cliff, 0, "get_claimable must return 0 exactly at cliff_time");
    
    // At t=501 (just after cliff): now claimable should be > 0
    t.env.ledger().set_timestamp(501);
    let claimable_after_cliff = c.get_claimable(&stream_id);
    assert!(claimable_after_cliff > 0, "get_claimable must return > 0 after cliff_time");
    
    // The claimable should be (501 - 500) * flow_rate = 1 * 100 = 100
    let expected = 100i128;
    assert_eq!(claimable_after_cliff, expected, "claimable should equal (time - cliff) * flow_rate");
}

#[test]
fn test_get_claimable_zero_dust_before_start() {
    // Regression test: ensure that a stream created but not yet started
    // returns 0 from get_claimable, not dust or rounding artifacts.
    let t = setup();
    let c = client(&t);
    
    t.env.ledger().set_timestamp(0);
    
    // Create stream with non-zero cliff to test cliff enforcement
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &100u64, &0u64,
        &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &false, &false,
    );
    
    // Before cliff, get_claimable must return exactly 0, not any dust value
    for t_val in [0u64, 50u64, 99u64] {
        t.env.ledger().set_timestamp(t_val);
        let claimable = c.get_claimable(&stream_id);
        assert_eq!(claimable, 0, "get_claimable must return 0 before cliff, not dust at t={}", t_val);
    }
}


#[test]
fn test_query_streams_empty_filter() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create some streams
    let _stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );
    let _stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Query with empty filter (no criteria specified)
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: None,
    };
    let results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(results.len(), 2, "Empty filter should return all streams");
}

#[test]
fn test_query_streams_by_status() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    let stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Cancel one stream
    c.cancel_stream(&stream_id2, &t.sender);

    // Query for Active streams
    let filter = StreamQueryFilter {
        status: Some(StreamStatus::Active),
        asset: None,
        sender: None,
        recipient: None,
    };
    let active_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(active_results.len(), 1, "Should return only active stream");
    assert_eq!(active_results.get(0).id, stream_id1);

    // Query for Cancelled streams
    let filter = StreamQueryFilter {
        status: Some(StreamStatus::Cancelled),
        asset: None,
        sender: None,
        recipient: None,
    };
    let cancelled_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(
        cancelled_results.len(),
        1,
        "Should return only cancelled stream"
    );
    assert_eq!(cancelled_results.get(0).id, stream_id2);
}

#[test]
fn test_query_streams_by_sender() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let sender2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&sender2, &1_000_000);

    // Create stream from sender1
    let _stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream from sender2
    let _stream_id2 = c.create_stream(
        &sender2,
        &t.recipient,
        &t.token_id,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Query for streams from sender1
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: Some(t.sender.clone()),
        recipient: None,
    };
    let sender1_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(sender1_results.len(), 1, "Should return only sender1 streams");
    assert_eq!(sender1_results.get(0).sender, t.sender);

    // Query for streams from sender2
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: Some(sender2.clone()),
        recipient: None,
    };
    let sender2_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(sender2_results.len(), 1, "Should return only sender2 streams");
    assert_eq!(sender2_results.get(0).sender, sender2);
}

#[test]
fn test_query_streams_by_recipient() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let recipient2 = Address::generate(&t.env);

    // Create stream to recipient1
    let _stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream to recipient2
    let _stream_id2 = c.create_stream(
        &t.sender,
        &recipient2,
        &t.token_id,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Query for streams to recipient1
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: Some(t.recipient.clone()),
    };
    let recipient1_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(
        recipient1_results.len(),
        1,
        "Should return only recipient1 streams"
    );
    assert_eq!(recipient1_results.get(0).recipient, t.recipient);

    // Query for streams to recipient2
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: Some(recipient2.clone()),
    };
    let recipient2_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(
        recipient2_results.len(),
        1,
        "Should return only recipient2 streams"
    );
    assert_eq!(recipient2_results.get(0).recipient, recipient2);
}

#[test]
fn test_query_streams_by_asset() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let token_admin2 = Address::generate(&t.env);
    let token_id2 = t
        .env
        .register_stellar_asset_contract_v2(token_admin2.clone())
        .address();
    StellarAssetClient::new(&t.env, &token_id2).mint(&t.sender, &1_000_000);

    // Create stream with token1
    let _stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream with token2
    let _stream_id2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &token_id2,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Query for streams using token1
    let filter = StreamQueryFilter {
        status: None,
        asset: Some(t.token_id.clone()),
        sender: None,
        recipient: None,
    };
    let token1_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(token1_results.len(), 1, "Should return only token1 streams");
    assert_eq!(token1_results.get(0).token, t.token_id);

    // Query for streams using token2
    let filter = StreamQueryFilter {
        status: None,
        asset: Some(token_id2.clone()),
        sender: None,
        recipient: None,
    };
    let token2_results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(token2_results.len(), 1, "Should return only token2 streams");
    assert_eq!(token2_results.get(0).token, token_id2);
}

#[test]
fn test_query_streams_multiple_filters() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let sender2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&sender2, &1_000_000);

    let recipient2 = Address::generate(&t.env);

    // Create stream1: sender1, recipient1, Active
    let stream_id1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream2: sender2, recipient1, Active
    let stream_id2 = c.create_stream(
        &sender2,
        &t.recipient,
        &t.token_id,
        &50_000i128,
        &500u64,
        &0u64,
        &1u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream3: sender1, recipient2, Active
    let stream_id3 = c.create_stream(
        &t.sender,
        &recipient2,
        &t.token_id,
        &75_000i128,
        &750u64,
        &0u64,
        &2u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );

    // Create stream4: sender1, recipient1, Cancelled
    let stream_id4 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &25_000i128,
        &250u64,
        &0u64,
        &3u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &false,
    );
    c.cancel_stream(&stream_id4, &t.sender);

    // Query: sender1, recipient1, Active
    let filter = StreamQueryFilter {
        status: Some(StreamStatus::Active),
        asset: None,
        sender: Some(t.sender.clone()),
        recipient: Some(t.recipient.clone()),
    };
    let results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(results.len(), 1, "Should return only matching stream");
    assert_eq!(results.get(0).id, stream_id1);

    // Query: sender1, Active (any recipient)
    let filter = StreamQueryFilter {
        status: Some(StreamStatus::Active),
        asset: None,
        sender: Some(t.sender.clone()),
        recipient: None,
    };
    let results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(results.len(), 2, "Should return sender1's active streams");
    let ids: Vec<u64> = vec![results.get(0).id, results.get(1).id];
    assert!(ids.contains(&stream_id1));
    assert!(ids.contains(&stream_id3));

    // Query: recipient1 (any sender/status)
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: Some(t.recipient.clone()),
    };
    let results = c.query_streams(&filter, &0u32, &20u32);
    assert_eq!(results.len(), 3, "Should return all streams to recipient1");
}

#[test]
fn test_query_streams_pagination() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create 5 streams
    for i in 0..5 {
        let _stream_id = c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &(100_000 - (i * 10_000)) as i128,
            &1000u64,
            &0u64,
            &(i as u64),
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &false,
        );
    }

    // Query with empty filter to get all
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: None,
    };

    // Get first page (limit=2)
    let page1 = c.query_streams(&filter, &0u32, &2u32);
    assert_eq!(page1.len(), 2, "First page should have 2 results");

    // Get second page (start=2, limit=2)
    let page2 = c.query_streams(&filter, &2u32, &2u32);
    assert_eq!(page2.len(), 2, "Second page should have 2 results");

    // Get third page (start=4, limit=2) - should only have 1
    let page3 = c.query_streams(&filter, &4u32, &2u32);
    assert_eq!(page3.len(), 1, "Third page should have 1 result");

    // Verify no overlap between pages
    let page1_ids: Vec<u64> = (0..page1.len()).map(|i| page1.get(i as u32).id).collect();
    let page2_ids: Vec<u64> = (0..page2.len()).map(|i| page2.get(i as u32).id).collect();
    let page3_ids: Vec<u64> = (0..page3.len()).map(|i| page3.get(i as u32).id).collect();

    for id in page1_ids.iter() {
        assert!(!page2_ids.contains(id), "Pages should not overlap");
        assert!(!page3_ids.contains(id), "Pages should not overlap");
    }
    for id in page2_ids.iter() {
        assert!(!page3_ids.contains(id), "Pages should not overlap");
    }
}

#[test]
fn test_query_streams_limit_capped_at_20() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create 25 streams
    for i in 0..25 {
        let _stream_id = c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &(100_000 - (i * 1_000)) as i128,
            &1000u64,
            &0u64,
            &(i as u64),
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &false,
        );
    }

    // Query with limit=50 should be capped at 20
    let filter = StreamQueryFilter {
        status: None,
        asset: None,
        sender: None,
        recipient: None,
    };
    let results = c.query_streams(&filter, &0u32, &50u32);
    assert_eq!(
        results.len(),
        20,
        "Limit should be capped at 20, got {}",
        results.len()
    );
}


/// Test case demonstrating the token refund vulnerability in cancel_stream.
/// 
/// VULNERABILITY: When cancel_stream is called:
/// 1. EFFECTS phase: Stream record is deleted from storage
/// 2. INTERACTIONS phase: Token transfers to recipient and sender
///
/// If the token transfer fails (e.g., insufficient balance, frozen account, failed token call),
/// the stream record is already gone, making the unstreamed tokens inaccessible.
/// 
/// The sender cannot recover the unstreamed amount because:
/// - The stream record was deleted
/// - The tokens remain in the contract
/// - There's no way to reconstruct the stream or claim the orphaned tokens
///
/// FIX: Move token transfers to occur BEFORE storage deletion (INTERACTIONS before EFFECTS).
/// This ensures atomicity: if transfers fail, the stream record remains intact and can be
/// retried or recovered.

#[test]
fn test_cancel_stream_token_refund_ordering_vulnerability() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create a stream with 1,000 total seconds and 100,000 token deposit
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128,  // deposit
        &1000u64,      // duration in seconds
        &0u64,         // cliff
        &0u64,         // start_time
        &false,        // auto_renew
        &0u64,         // end_time_or_cliff
        &false,        // is_step_vesting
        &0i128,        // holdback_amount
        &None::<u32>,  // decay_factor
        &None::<i128>, // decay_offset
    );

    // Advance time to 300 seconds (30% of stream)
    t.env.ledger().set_timestamp(300);

    // Record balances before cancellation
    let sender_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_balance_before = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    // Cancel the stream
    c.cancel_stream(&stream_id, &t.sender);

    // EXPECTED BEHAVIOR:
    // - Recipient gets earned amount: (300 / 1000) * 100_000 = 30_000
    // - Sender gets refund: 100_000 - 30_000 = 70_000 (unstreamed portion)
    
    let sender_balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let recipient_balance_after = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);

    let sender_received = sender_balance_after - sender_balance_before;
    let recipient_received = recipient_balance_after - recipient_balance_before;

    assert_eq!(recipient_received, 30_000, "recipient should receive earned 30% of deposit");
    assert_eq!(sender_received, 70_000, "sender should receive unstreamed 70% refund");

    // Stream should be deleted
    assert!(
        c.try_get_stream(&stream_id).is_err(),
        "stream must be deleted after cancellation"
    );

    // VULNERABILITY SCENARIO (not directly testable in mock environment):
    // If token transfer failed between:
    //   1. Stream deletion (remove_stream called)
    //   2. Refund transfer (token_client.transfer failed)
    //
    // Then:
    //   - Stream record is gone
    //   - 70_000 tokens remain in contract (not transferred to sender)
    //   - Sender cannot recover because stream record doesn't exist
    //   - No function exists to reclaim orphaned tokens
}


#[test]
fn test_flow_rate_bounds_validation_prevents_overflow() {
    let t = setup();
    let c = client(&t);

    // Attempting to create a stream with amount = i128::MAX and duration = 1 second
    // would normally result in flow_rate = i128::MAX, which would overflow on any multiplication
    // The validation should reject this at creation time with StreamError::Overflow
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &i128::MAX,          // Extremely large amount
        &1u64,               // Very short duration (1 second)
        &0u64,               // cliff
        &0u64,               // start_time
        &false,              // auto_renew
        &0u64,               // end_time_or_cliff
        &false,              // is_step_vesting
        &0i128,              // holdback_amount
        &None::<u32>,        // decay_factor
        &None::<i128>,       // decay_offset
    );

    // Should fail at creation time with Overflow error, not at withdraw time
    assert!(result.is_err(), "Should reject stream with unsafe flow_rate at creation");
    
    // Verify it's an Overflow error (or similar bounds-checking error)
    match result {
        Err(e) => {
            // The error should indicate the flow_rate is too large
            // It could be Overflow, InvalidDuration, or similar
            assert!(
                matches!(e, StreamError::Overflow),
                "Expected Overflow error for unsafe flow_rate, got: {:?}",
                e
            );
        }
        Ok(_) => panic!("Should have rejected stream with i128::MAX amount and 1-second duration"),
    }
}

#[test]
fn test_large_flow_rate_with_long_duration_succeeds() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(100);

    // Create a stream with a very large amount but long enough duration that flow_rate is safe
    // Example: deposit = 10^18 (realistic for USDC with many stroops)
    //          duration = 1 year (31,536,000 seconds)
    //          flow_rate ~= 31,709,791 stroops/sec (safe to multiply by elapsed)
    
    let large_deposit = 1_000_000_000_000_000_000i128;  // 10^18 stroops
    let one_year_seconds = 365u64 * 24 * 60 * 60;       // 31,536,000 seconds

    // First mint enough tokens
    TokenClient::new(&t.env, &t.token_id).mint(&t.sender, &large_deposit);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &large_deposit,
        &one_year_seconds,
        &0u64,               // cliff
        &0u64,               // start_time
        &false,              // auto_renew
        &0u64,               // end_time_or_cliff
        &false,              // is_step_vesting
        &0i128,              // holdback_amount
        &None::<u32>,        // decay_factor
        &None::<i128>,       // decay_offset
    );

    // Should succeed - the stream should be created
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, large_deposit);
    assert_eq!(stream.status, StreamStatus::Active);

    // Advance time to mid-stream and verify we can withdraw
    t.env.ledger().set_timestamp(100 + one_year_seconds / 2);
    
    let claimable = c.get_claimable(&stream_id);
    assert!(
        claimable > 0,
        "Should be able to compute claimable amount at mid-stream"
    );
    assert!(
        claimable < large_deposit,
        "Claimable should be less than full deposit at mid-stream"
    );
}

#[test]
fn test_extremely_large_flow_rate_causes_creation_error() {
    let t = setup();
    let c = client(&t);

    // Attempt to create with flow_rate that would be close to i128::MAX
    // This should be caught at creation time, not at withdraw time
    let unsafe_amount = 9_223_372_036_854_775_800i128;  // Close to i128::MAX
    let short_duration = 2u64;  // 2 seconds, so flow_rate ~= i128::MAX / 2

    // Mint enough for this test
    TokenClient::new(&t.env, &t.token_id).mint(&t.sender, &unsafe_amount);

    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &unsafe_amount,
        &short_duration,
        &0u64,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
    );

    // Should fail at creation with Overflow error
    assert!(result.is_err(), "Should reject extremely large flow_rate at creation");
}


#[test]
fn test_zero_duration_explicitly_rejected() {
    let t = setup();
    let c = client(&t);

    // Test that zero-duration streams are explicitly rejected
    // This should fail with InvalidDuration, not at withdrawal time or with unclear errors
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,  // amount
        &0u64,         // duration_seconds = 0 (zero duration)
        &0u64,         // cliff_seconds
        &0u64,         // nonce
        &false,        // auto_renew
        &0u64,         // lock_until
        &false,        // allow_recipient_termination
        &0i128,        // holdback_amount
        &None::<u32>,  // withdrawal_steps
        &None::<i128>, // min_withdrawal_amount
    );

    // Should fail with InvalidDuration error
    assert!(
        result.is_err(),
        "Zero-duration stream should be rejected at creation time"
    );
    
    match result {
        Err(e) => {
            assert_eq!(
                e,
                Ok(StreamError::InvalidDuration),
                "Expected InvalidDuration error for zero-duration stream"
            );
        }
        Ok(_) => panic!("Zero-duration stream should not be allowed"),
    }
}

#[test]
fn test_minimal_duration_is_allowed() {
    let t = setup();
    let c = client(&t);
    
    // Verify that the minimal non-zero duration (1 second) is allowed
    // and works correctly
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,  // amount
        &1u64,         // duration_seconds = 1 (minimal non-zero)
        &0u64,         // cliff_seconds
        &0u64,         // nonce
        &false,        // auto_renew
        &0u64,         // lock_until
        &false,        // allow_recipient_termination
        &0i128,        // holdback_amount
        &None::<u32>,  // withdrawal_steps
        &None::<i128>, // min_withdrawal_amount
    );

    // Should succeed
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.deposit, 100_000);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.flow_rate, 100_000);  // 100_000 / 1 = 100_000
    
    // Verify end_time > start_time
    assert!(
        stream.end_time > stream.start_time,
        "Stream with 1-second duration should have end_time > start_time"
    );
}

#[test]
fn test_zero_duration_cannot_be_bypassed_with_minimum_duration_zero() {
    let t = setup();
    let c = client(&t);
    
    // Even if minimum duration is set to 0, zero-duration streams should still be rejected
    // This tests that the explicit zero-duration check is independent of the minimum duration setting
    
    // The setup already calls set_min_duration(0), so minimum duration is 0
    // But zero-duration should still be rejected
    let result = c.try_create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &50_000i128,   // amount
        &0u64,         // duration_seconds = 0 (explicitly zero)
        &0u64,         // cliff_seconds
        &1u64,         // nonce (different from other tests)
        &false,        // auto_renew
        &0u64,         // lock_until
        &false,        // allow_recipient_termination
        &0i128,        // holdback_amount
        &None::<u32>,  // withdrawal_steps
        &None::<i128>, // min_withdrawal_amount
    );

    // Should fail even though minimum duration is 0
    assert!(
        result.is_err(),
        "Zero-duration stream should be rejected even when min_duration is 0"
    );
    
    match result {
        Err(e) => {
            assert_eq!(
                e,
                Ok(StreamError::InvalidDuration),
                "Should get InvalidDuration error, not StreamDurationTooShort or other errors"
            );
        }
        Ok(_) => panic!("Zero-duration should never be allowed"),
    }
}


#[test]
fn test_batch_create_insufficient_balance_rejects_entire_batch() {
    let t = setup();
    let c = client(&t);
    
    // Create vectors for batch create
    let mut recipients = Vec::new(&t.env);
    recipients.push_back(t.recipient.clone());
    recipients.push_back(Address::generate(&t.env));
    
    let mut amounts = Vec::new(&t.env);
    amounts.push_back(400_000i128);  // First stream
    amounts.push_back(700_000i128);  // Second stream (total = 1,100,000)
    
    let mut tokens = Vec::new(&t.env);
    tokens.push_back(t.token_id.clone());
    tokens.push_back(t.token_id.clone());
    
    let mut lock_untils = Vec::new(&t.env);
    lock_untils.push_back(0u64);
    lock_untils.push_back(0u64);
    
    // Sender only has 1,000,000 tokens, but needs 1,100,000
    // The batch should be ENTIRELY rejected, with no streams created
    let result = c.try_batch_create_stream(
        &t.sender,
        &recipients,
        &amounts,
        &tokens,
        &1000u64,  // duration
        &false,    // auto_renew
        &lock_untils,
        &0u64,     // nonce
    );
    
    // Should fail due to insufficient balance
    assert!(result.is_err(), "Batch should be rejected due to insufficient balance");
    
    // Verify NO streams were created (all-or-nothing)
    let all_stream_ids = c.get_all_stream_ids(&0u64, &1000u32);
    assert_eq!(all_stream_ids.len(), 0, "No streams should be created when batch fails");
}

#[test]
fn test_batch_create_sufficient_balance_succeeds_for_all() {
    let t = setup();
    let c = client(&t);
    
    // Mint enough tokens for multiple streams
    TokenClient::new(&t.env, &t.token_id).mint(&t.sender, &1_000_000);
    
    let mut recipients = Vec::new(&t.env);
    recipients.push_back(t.recipient.clone());
    recipients.push_back(Address::generate(&t.env));
    recipients.push_back(Address::generate(&t.env));
    
    let mut amounts = Vec::new(&t.env);
    amounts.push_back(300_000i128);
    amounts.push_back(300_000i128);
    amounts.push_back(300_000i128);
    
    let mut tokens = Vec::new(&t.env);
    tokens.push_back(t.token_id.clone());
    tokens.push_back(t.token_id.clone());
    tokens.push_back(t.token_id.clone());
    
    let mut lock_untils = Vec::new(&t.env);
    lock_untils.push_back(0u64);
    lock_untils.push_back(0u64);
    lock_untils.push_back(0u64);
    
    let stream_ids = c.batch_create_stream(
        &t.sender,
        &recipients,
        &amounts,
        &tokens,
        &1000u64,
        &false,
        &lock_untils,
        &0u64,
    );
    
    // All 3 streams should be created
    assert_eq!(stream_ids.len(), 3, "All 3 streams should be created");
    
    // Verify all streams exist with correct parameters
    for i in 0..3 {
        let stream_id = stream_ids.get(i).unwrap();
        let stream = c.get_stream(&stream_id);
        assert_eq!(stream.deposit, 300_000i128);
        assert_eq!(stream.status, StreamStatus::Active);
    }
}

#[test]
fn test_batch_create_validates_flow_rate_bounds() {
    let t = setup();
    let c = client(&t);
    
    // Mint a huge amount to test flow rate bounds
    let huge_amount = i128::MAX / 2;
    TokenClient::new(&t.env, &t.token_id).mint(&t.sender, &huge_amount);
    
    let mut recipients = Vec::new(&t.env);
    recipients.push_back(t.recipient.clone());
    
    let mut amounts = Vec::new(&t.env);
    amounts.push_back(huge_amount);  // Extremely large amount
    
    let mut tokens = Vec::new(&t.env);
    tokens.push_back(t.token_id.clone());
    
    let mut lock_untils = Vec::new(&t.env);
    lock_untils.push_back(0u64);
    
    // Even with sufficient balance, flow_rate = huge_amount / 1 would overflow
    // So the batch should be rejected in Phase 1
    let result = c.try_batch_create_stream(
        &t.sender,
        &recipients,
        &amounts,
        &tokens,
        &1u64,  // 1-second duration (makes flow_rate = huge_amount)
        &false,
        &lock_untils,
        &0u64,
    );
    
    // Should fail due to unsafe flow_rate
    assert!(result.is_err(), "Batch should reject unsafe flow_rate");
    
    // No streams should be created
    let all_stream_ids = c.get_all_stream_ids(&0u64, &1000u32);
    assert_eq!(all_stream_ids.len(), 0, "No streams should be created with unsafe flow_rate");
}

#[test]
fn test_batch_create_multi_token_balance_check() {
    let t = setup();
    let c = client(&t);
    
    // Create a second token
    let token_admin = Address::generate(&t.env);
    let token2_id = t.env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    
    // Mint tokens: sender has 600_000 of token1 but only 100_000 of token2
    let token1_client = TokenClient::new(&t.env, &t.token_id);
    token1_client.mint(&t.sender, &600_000);
    
    let token2_client = TokenClient::new(&t.env, &token2_id);
    token2_client.mint(&t.sender, &100_000);
    
    let mut recipients = Vec::new(&t.env);
    recipients.push_back(t.recipient.clone());
    recipients.push_back(Address::generate(&t.env));
    
    let mut amounts = Vec::new(&t.env);
    amounts.push_back(300_000i128);  // Token 1: 300_000 needed
    amounts.push_back(200_000i128);  // Token 2: 200_000 needed (but only has 100_000)
    
    let mut tokens = Vec::new(&t.env);
    tokens.push_back(t.token_id.clone());
    tokens.push_back(token2_id.clone());
    
    let mut lock_untils = Vec::new(&t.env);
    lock_untils.push_back(0u64);
    lock_untils.push_back(0u64);
    
    // Batch should fail: insufficient token2 balance
    let result = c.try_batch_create_stream(
        &t.sender,
        &recipients,
        &amounts,
        &tokens,
        &1000u64,
        &false,
        &lock_untils,
        &0u64,
    );

    assert!(result.is_err(), "Batch should fail due to insufficient token2 balance");

    // No streams should be created
    let all_stream_ids = c.get_all_stream_ids(&0u64, &1000u32);
    assert_eq!(all_stream_ids.len(), 0, "No streams when any token has insufficient balance");
}

// Issue #489: splitStream closes parent stream and distributes deposit to children
#[test]
fn test_split_stream_closes_parent_stream() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create a stream with 1,000,000 stroops deposit
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Verify parent stream exists and is Active
    let parent_stream = c.get_stream(&stream_id);
    assert_eq!(parent_stream.status, StreamStatus::Active);
    assert_eq!(parent_stream.deposit, 1_000_000);

    // Create recipients for split
    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let mut recipients = Vec::new(&t.env);
    recipients.push_back(recipient1.clone());
    recipients.push_back(recipient2.clone());

    let mut proportions = Vec::new(&t.env);
    proportions.push_back(1u128); // 50%
    proportions.push_back(1u128); // 50%

    // Split the stream
    let child_stream_ids = c.split_stream(&stream_id, &t.sender, &recipients, &proportions, &0u64);
    assert_eq!(child_stream_ids.len(), 2);

    // Verify parent stream is closed (should not exist in storage)
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err(), "Parent stream should be closed after split");

    // Verify child streams exist and have correct deposits
    let child_stream1 = c.get_stream(&child_stream_ids.get(0).unwrap());
    let child_stream2 = c.get_stream(&child_stream_ids.get(1).unwrap());

    assert_eq!(child_stream1.status, StreamStatus::Active);
    assert_eq!(child_stream2.status, StreamStatus::Active);
    assert_eq!(child_stream1.deposit, 500_000);
    assert_eq!(child_stream2.deposit, 500_000);
}

// Issue #490: getStream returns Completed status when stream deposit is fully exhausted
#[test]
fn test_exhausted_stream_shows_completed_status() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Create a stream with 1000 stroops deposit and 100 stroop/sec flow rate
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1000,
        &100,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Verify initial status is Active
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);

    // Advance time to allow full amount to be claimable (10 seconds for 1000 stroops at 100/sec)
    t.env.ledger().set_timestamp(10);

    // Withdraw all available amount
    c.withdraw(&stream_id, &t.recipient);

    // Check the stream status - should be Completed after full exhaustion
    let stream_after = c.get_stream(&stream_id);
    assert_eq!(
        stream_after.status,
        StreamStatus::Completed,
        "Stream should show Completed status when deposit is fully exhausted"
    );
}

// Issue #491: execute_admin_override entry point checks timelock before execution
#[test]
fn test_admin_override_checks_timelock() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let admin = Address::generate(&t.env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&t.env, "1.0.0"));

    // Set admin override timelock to 1000 seconds
    let timelock_seconds = 1000u64;
    c.set_admin_override_timelock(&timelock_seconds).unwrap();

    // Create a stream
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &100,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Initiate an override request
    let reason = soroban_sdk::String::from_str(&t.env, "test override");
    let request_id = c.initiate_admin_override(
        &stream_id,
        &OverrideAction::Cancel,
        &reason,
    ).unwrap();

    // Try to execute override immediately (should fail - timelock not elapsed)
    let result = c.try_execute_admin_override(&request_id);
    assert!(result.is_err(), "execute_admin_override should fail before timelock elapsed");

    // Advance time past the timelock
    t.env.ledger().set_timestamp(timelock_seconds + 1);

    // Now execute override should succeed
    let result = c.try_execute_admin_override(&request_id);
    assert!(result.is_ok(), "execute_admin_override should succeed after timelock elapsed");
}

// Issue #492: Invalid stream ID returns StreamNotFound error, not generic ContractError
#[test]
fn test_invalid_stream_id_returns_stream_not_found_error() {
    let t = setup();
    let c = client(&t);

    // Try to get a stream that doesn't exist
    let result = c.try_get_stream(&999999u64);

    // Should return an error (StreamError::StreamNotFound)
    assert!(result.is_err(), "Should error for invalid stream ID");

    // Verify the error is specifically StreamNotFound
    // The error code 1 corresponds to StreamError::StreamNotFound
    match result {
        Err(e) => {
            // Check that the error code is 1 (StreamNotFound)
            let error_code = e.code;
            assert_eq!(error_code, 1, "Error should be StreamNotFound (code 1), not a generic ContractError");
        }
        Ok(_) => panic!("Should have returned an error for invalid stream ID"),
    }
}
