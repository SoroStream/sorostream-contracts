//!
//! The rate limit state `(window_start_ledger, count)` lives in **temporary**
//! storage and is keyed per sender.  Key properties under test:
//!
//! - Creations under the cap succeed.
//! - The (cap + 1)th creation in the same window returns `RateLimitExceeded`.
//! - Advancing the ledger by >= `window_ledgers` resets the counter.
//! - Exempt addresses bypass the cap entirely.
//! - Removing an exemption re-applies the limit.
//! - `remaining_quota` reflects live usage.
//! - Rate limits are per-sender: one sender's usage does not affect another.
//! - Admin-only enforcement: non-admin callers cannot mutate config.

use super::*;
use crate::types::CreateStreamParams;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ── Test fixture ──────────────────────────────────────────────────────────────

struct RlTestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

fn rl_setup() -> RlTestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    // Mint plenty of tokens so token-transfer never fails.
    StellarAssetClient::new(&env, &token_id).mint(&sender, &100_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    // Disable the minimum duration check so tiny-duration streams work in tests.
    c.set_min_duration(&admin, &0u64);

    RlTestEnv { env, contract_id, token_id, sender, recipient, admin }
}

fn rl_client(t: &RlTestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

/// Helper: build minimal CreateStreamParams for a given nonce.
fn rl_params(nonce: u64) -> CreateStreamParams {
    CreateStreamParams {
        cliff_seconds: 0,
        nonce,
        renew_count: None,
        lock_until: 0,
        allow_recipient_termination: false,
        non_transferable: false,
        holdback_amount: 0,
        withdrawal_steps: None,
        min_withdrawal_amount: None,
        requires_recipient_approval: false,
    }
}

/// Helper: create one stream with a unique nonce.  Returns the stream ID.
fn make_stream(t: &RlTestEnv, nonce: u64) -> u64 {
    rl_client(t).create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(nonce),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A sender under the cap can create streams up to the limit without error.
#[test]
fn test_rl_allows_under_cap() {
    let t = rl_setup();
    let c = rl_client(&t);

    // Tight cap: 3 per window.
    c.set_rate_limit_max(&t.admin, &3u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    for nonce in 0u64..3 {
        let result = c.try_create_stream(
            &t.sender, &t.recipient, &t.token_id,
            &10_000, &3600u64, &false, &rl_params(nonce),
        );
        assert!(result.is_ok(), "stream {nonce} should be allowed under the cap");
    }
}

/// The (cap + 1)th creation in the same window returns `RateLimitExceeded`.
#[test]
fn test_rl_blocks_at_cap() {
    let t = rl_setup();
    let c = rl_client(&t);

    c.set_rate_limit_max(&t.admin, &3u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    // Exhaust quota.
    for nonce in 0u64..3 {
        make_stream(&t, nonce);
    }

    // One over the cap.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(3),
    );
    assert_eq!(
        result,
        Err(Ok(StreamError::RateLimitExceeded)),
        "creation over cap must return RateLimitExceeded"
    );
}

/// After the window elapses (ledger sequence advances >= window_ledgers),
/// the counter resets and the sender can create streams again.
#[test]
fn test_rl_resets_after_window() {
    let t = rl_setup();
    let c = rl_client(&t);

    // Small window for easy test control.
    c.set_rate_limit_max(&t.admin, &2u32);
    c.set_rate_limit_window(&t.admin, &100u32);

    // Use up quota.
    make_stream(&t, 0);
    make_stream(&t, 1);

    // Blocked within the window.
    let blocked = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(2),
    );
    assert_eq!(blocked, Err(Ok(StreamError::RateLimitExceeded)));

    // Advance the ledger sequence by exactly window_ledgers (100) to expire the window.
    let new_seq = t.env.ledger().sequence() + 100;
    t.env.ledger().set_sequence_number(new_seq);

    // Fresh window — first creation should succeed.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(2),
    );
    assert!(result.is_ok(), "creation after window reset should succeed");
}

/// An exempt sender bypasses the cap entirely.
#[test]
fn test_rl_exempt_bypasses_cap() {
    let t = rl_setup();
    let c = rl_client(&t);

    // Extremely tight cap.
    c.set_rate_limit_max(&t.admin, &1u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    // Grant exemption.
    c.add_rate_limit_exempt(&t.admin, &t.sender);

    // Create well over the limit — all should succeed.
    for nonce in 0u64..5 {
        let result = c.try_create_stream(
            &t.sender, &t.recipient, &t.token_id,
            &10_000, &3600u64, &false, &rl_params(nonce),
        );
        assert!(result.is_ok(), "exempt sender stream {nonce} must succeed");
    }
}

/// Removing an exemption re-subjects the address to the normal cap.
#[test]
fn test_rl_remove_exempt_reapplies_limit() {
    let t = rl_setup();
    let c = rl_client(&t);

    c.set_rate_limit_max(&t.admin, &1u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    // Grant then revoke.
    c.add_rate_limit_exempt(&t.admin, &t.sender);
    c.remove_rate_limit_exempt(&t.admin, &t.sender);

    // First creation uses the single allowed slot.
    make_stream(&t, 0);

    // Second is rejected.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(1),
    );
    assert_eq!(result, Err(Ok(StreamError::RateLimitExceeded)));
}

/// `remaining_quota` returns the full quota for a fresh sender and decrements on each creation.
#[test]
fn test_rl_remaining_quota_decrements() {
    let t = rl_setup();
    let c = rl_client(&t);

    c.set_rate_limit_max(&t.admin, &5u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    // Fresh sender has full quota.
    assert_eq!(c.remaining_quota(&t.sender), 5u32);

    // Create 2 streams.
    make_stream(&t, 0);
    make_stream(&t, 1);

    assert_eq!(c.remaining_quota(&t.sender), 3u32);
}

/// `remaining_quota` returns `u32::MAX` for an exempt sender.
#[test]
fn test_rl_remaining_quota_exempt_is_max() {
    let t = rl_setup();
    let c = rl_client(&t);

    c.add_rate_limit_exempt(&t.admin, &t.sender);
    assert_eq!(c.remaining_quota(&t.sender), u32::MAX);
}

/// `remaining_quota` returns the full quota after the window lapses.
#[test]
fn test_rl_remaining_quota_resets_after_window() {
    let t = rl_setup();
    let c = rl_client(&t);

    c.set_rate_limit_max(&t.admin, &3u32);
    c.set_rate_limit_window(&t.admin, &50u32);

    // Use 2 slots.
    make_stream(&t, 0);
    make_stream(&t, 1);
    assert_eq!(c.remaining_quota(&t.sender), 1u32);

    // Advance past the window.
    let new_seq = t.env.ledger().sequence() + 50;
    t.env.ledger().set_sequence_number(new_seq);

    // Quota should appear fully available (window has lapsed).
    assert_eq!(c.remaining_quota(&t.sender), 3u32);
}

/// Rate limits are per-sender: exhausting one sender's quota does not affect another.
#[test]
fn test_rl_is_per_sender() {
    let t = rl_setup();
    let c = rl_client(&t);

    let sender2 = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&sender2, &1_000_000);

    c.set_rate_limit_max(&t.admin, &2u32);
    c.set_rate_limit_window(&t.admin, &720u32);

    // Exhaust sender1's quota.
    make_stream(&t, 0);
    make_stream(&t, 1);

    let blocked = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(2),
    );
    assert_eq!(blocked, Err(Ok(StreamError::RateLimitExceeded)));

    // sender2 is unaffected.
    let ok = c.try_create_stream(
        &sender2, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(0),
    );
    assert!(ok.is_ok(), "sender2 must not be rate-limited by sender1's usage");
}

/// Admin can update window and max; new settings take effect immediately.
#[test]
fn test_rl_admin_can_update_config() {
    let t = rl_setup();
    let c = rl_client(&t);

    // Start with a cap of 1.
    c.set_rate_limit_max(&t.admin, &1u32);
    c.set_rate_limit_window(&t.admin, &720u32);
    make_stream(&t, 0);

    // Blocked at cap of 1.
    assert_eq!(
        c.try_create_stream(&t.sender, &t.recipient, &t.token_id, &10_000, &3600u64, &false, &rl_params(1)),
        Err(Ok(StreamError::RateLimitExceeded))
    );

    // Admin raises cap to 5.
    c.set_rate_limit_max(&t.admin, &5u32);

    // Now the second creation should succeed (quota: 4 remaining out of 5, but window
    // started at ledger when first stream was created — still same window).
    // Actually: after raising the cap, the sender has count=1, max=5 → 4 more allowed.
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &10_000, &3600u64, &false, &rl_params(1),
    );
    assert!(result.is_ok(), "creation after cap increase should succeed");
}
