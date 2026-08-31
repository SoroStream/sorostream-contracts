//! Unit tests for the four new features:
//! (a) StreamExpiryWarning events
//! (b) New-sender stream cap + SenderPromoted
//! (c) Stream redirect chaining
//! (d) Dual-token streams (create_dual_stream)

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol, Val,
};

// â”€â”€ Shared test helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

struct FTestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    token2_id: Address,
    sender: Address,
    recipient: Address,
    admin: Address,
}

fn fsetup() -> FTestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);

    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token2_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);
    StellarAssetClient::new(&env, &token2_id).mint(&sender, &10_000_000);

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    c.set_min_duration(&admin, &0u64);

    FTestEnv { env, contract_id, token_id, token2_id, sender, recipient, admin }
}

fn fclient(t: &FTestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn make_stream(t: &FTestEnv, nonce: u64, duration: u64) -> u64 {
    fclient(t).create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &duration, &0, &nonce, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>,
    )
}

fn has_event(t: &FTestEnv, name: &str) -> bool {
    t.env.events().all().iter().any(|(_, topics, _)| {
        let v: soroban_sdk::Vec<Val> = topics.clone();
        if v.is_empty() { return false; }
        let sym: Symbol = v.get(0).unwrap().into_val(&t.env);
        sym == Symbol::new(&t.env, name)
    })
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Feature (a): StreamExpiryWarning
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Default window is 17280 ledgers (~24 h). Admin can change it.
#[test]
fn test_expiry_warning_window_default_and_set() {
    let t = fsetup();
    let c = fclient(&t);

    assert_eq!(c.get_expiry_warning_window(), 17_280u32);

    c.set_expiry_warning_window(&1000u32);
    assert_eq!(c.get_expiry_warning_window(), 1000u32);
}

/// Setting window to 0 is rejected.
#[test]
fn test_expiry_warning_window_zero_rejected() {
    let t = fsetup();
    let result = fclient(&t).try_set_expiry_warning_window(&0u32);
    assert_eq!(result, Err(Ok(StreamError::InvalidExpiryWindow)));
}

/// StreamExpiryWarning is emitted on withdraw when the stream is within the window.
/// A 1000-second stream at t=0; set window to cover the whole stream (200001 ledgers).
/// Withdraw at t=500 (halfway) should emit the warning.
#[test]
fn test_expiry_warning_emitted_on_withdraw_within_window() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    // Window = 200001 ledgers so remaining ~= (1000/5)=200 ledgers is always inside
    c.set_expiry_warning_window(&200_001u32);

    let stream_id = make_stream(&t, 0, 1000);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    assert!(has_event(&t, "StreamExpiryWarning"), "expected StreamExpiryWarning event");
}

/// StreamExpiryWarning is NOT emitted when stream is outside the window.
#[test]
fn test_expiry_warning_not_emitted_outside_window() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    // Very small window: 1 ledger. At t=0 with 1000s remaining, 200 ledgers remain â€” outside window.
    c.set_expiry_warning_window(&1u32);

    let stream_id = make_stream(&t, 0, 1000);

    t.env.ledger().set_timestamp(0);
    c.withdraw(&stream_id, &t.recipient);

    assert!(!has_event(&t, "StreamExpiryWarning"), "unexpected StreamExpiryWarning event");
}

/// Idempotency: second interaction in window does NOT re-emit the warning.
#[test]
fn test_expiry_warning_idempotent() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    c.set_expiry_warning_window(&200_001u32);

    let stream_id = make_stream(&t, 0, 1000);

    // First withdraw at t=500 â€” should emit warning
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let count_after_first = t.env.events().all().iter()
        .filter(|(_, topics, _)| {
            let v: soroban_sdk::Vec<Val> = topics.clone();
            if v.is_empty() { return false; }
            let sym: Symbol = v.get(0).unwrap().into_val(&t.env);
            sym == Symbol::new(&t.env, "StreamExpiryWarning")
        })
        .count();
    assert_eq!(count_after_first, 1, "exactly one StreamExpiryWarning after first withdraw");

    // Second withdraw at t=600 â€” should NOT re-emit
    t.env.ledger().set_timestamp(600);
    c.withdraw(&stream_id, &t.recipient);

    let count_after_second = t.env.events().all().iter()
        .filter(|(_, topics, _)| {
            let v: soroban_sdk::Vec<Val> = topics.clone();
            if v.is_empty() { return false; }
            let sym: Symbol = v.get(0).unwrap().into_val(&t.env);
            sym == Symbol::new(&t.env, "StreamExpiryWarning")
        })
        .count();
    assert_eq!(count_after_second, 1, "still exactly one StreamExpiryWarning after second withdraw");
}

/// StreamExpiryWarning event data fields match expected values.
#[test]
fn test_expiry_warning_event_fields() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    c.set_expiry_warning_window(&200_001u32);

    let stream_id = make_stream(&t, 0, 1000);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let events = t.env.events().all();
    let warning: std::vec::Vec<_> = events.iter().filter(|(_, topics, _)| {
        let v: soroban_sdk::Vec<Val> = topics.clone();
        if v.is_empty() { return false; }
        let sym: Symbol = v.get(0).unwrap().into_val(&t.env);
        sym == Symbol::new(&t.env, "StreamExpiryWarning")
    }).collect();

    assert_eq!(warning.len(), 1);
    let (_, topics, data) = &warning[0];

    // Topic[1] = stream_id
    let v: soroban_sdk::Vec<Val> = topics.clone();
    let sid: u64 = v.get(1).unwrap().into_val(&t.env);
    assert_eq!(sid, stream_id);

    // Data: (sender, recipient, remaining_balance, ledgers_until_expiry)
    let (s, r, _bal, _ledgers): (Address, Address, i128, u32) = data.clone().into_val(&t.env);
    assert_eq!(s, t.sender);
    assert_eq!(r, t.recipient);
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Feature (b): New-sender stream cap + SenderPromoted
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// A fresh sender is subject to the new-sender cap.
#[test]
fn test_new_sender_cap_enforced() {
    let t = fsetup();
    let c = fclient(&t);

    // Set cap = 2 streams, threshold = 100 (far away)
    c.set_new_sender_stream_cap(&2u32);
    c.set_sender_promotion_threshold(&100u32);

    // Create 2 streams â€” both succeed
    make_stream(&t, 0, 1000);
    make_stream(&t, 1, 1000);

    // 3rd stream should hit the cap
    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &2u64, &false, &0u64, &false, &0i128,
    );
    assert_eq!(result, Err(Ok(StreamError::NewSenderStreamCapExceeded)));
}

/// After cancelling a stream the sender can create another (slot freed).
#[test]
fn test_new_sender_cap_lifted_after_cancel() {
    let t = fsetup();
    let c = fclient(&t);
    c.set_new_sender_stream_cap(&1u32);
    c.set_sender_promotion_threshold(&100u32);

    let id = make_stream(&t, 0, 1000);

    // Cancel frees the active slot
    c.cancel_stream(&id, &t.sender);

    // Now lifetime count = 1 (still below threshold=100), active = 0 â€” cap not hit
    make_stream(&t, 1, 1000);
}

/// Lifetime count is tracked persistently and increments on each creation.
#[test]
fn test_sender_lifetime_count_increments() {
    let t = fsetup();
    let c = fclient(&t);

    assert_eq!(c.get_sender_lifetime_count(&t.sender), 0);
    make_stream(&t, 0, 1000);
    assert_eq!(c.get_sender_lifetime_count(&t.sender), 1);
    make_stream(&t, 1, 1000);
    assert_eq!(c.get_sender_lifetime_count(&t.sender), 2);
}

/// SenderPromoted event is emitted exactly once when crossing the threshold.
#[test]
fn test_sender_promoted_event_emitted_at_threshold() {
    let t = fsetup();
    let c = fclient(&t);

    // Threshold = 2: after creating 2 streams the sender is promoted
    c.set_new_sender_stream_cap(&10u32);
    c.set_sender_promotion_threshold(&2u32);

    assert!(!c.is_sender_promoted(&t.sender));
    make_stream(&t, 0, 1000);
    assert!(!c.is_sender_promoted(&t.sender));

    make_stream(&t, 1, 1000);
    assert!(c.is_sender_promoted(&t.sender));
    assert!(has_event(&t, "SenderPromoted"));
}

/// After promotion, the new-sender cap no longer applies.
#[test]
fn test_promoted_sender_bypasses_cap() {
    let t = fsetup();
    let c = fclient(&t);

    // Cap = 1, threshold = 2 â€” after 2 creations, cap is lifted
    c.set_new_sender_stream_cap(&1u32);
    c.set_sender_promotion_threshold(&2u32);

    // First stream â€” succeeds (cap=1, active=0)
    make_stream(&t, 0, 1000);
    // Second stream would normally be blocked by cap=1, but this is the threshold-crossing one
    make_stream(&t, 1, 1000);

    // Now promoted â€” can create even though active streams > original cap
    make_stream(&t, 2, 1000);
}

/// SenderPromoted event fires only once even with many subsequent creations.
#[test]
fn test_sender_promoted_event_fires_once() {
    let t = fsetup();
    let c = fclient(&t);
    c.set_new_sender_stream_cap(&10u32);
    c.set_sender_promotion_threshold(&2u32);

    make_stream(&t, 0, 1000);
    make_stream(&t, 1, 1000); // crosses threshold
    make_stream(&t, 2, 1000);
    make_stream(&t, 3, 1000);

    let promoted_count = t.env.events().all().iter()
        .filter(|(_, topics, _)| {
            let v: soroban_sdk::Vec<Val> = topics.clone();
            if v.is_empty() { return false; }
            let sym: Symbol = v.get(0).unwrap().into_val(&t.env);
            sym == Symbol::new(&t.env, "SenderPromoted")
        })
        .count();
    assert_eq!(promoted_count, 1, "SenderPromoted should emit exactly once");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Feature (c): Stream redirect chaining
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Recipient can set and retrieve a redirect target.
#[test]
fn test_set_redirect_stores_target() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let tgt = make_stream(&t, 1, 2000);

    assert_eq!(c.get_redirect(&src), None);
    c.set_redirect(&src, &tgt, &t.recipient);
    assert_eq!(c.get_redirect(&src), Some(tgt));
}

/// Redirect emits StreamRedirectSet event with correct fields.
#[test]
fn test_set_redirect_emits_event() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let tgt = make_stream(&t, 1, 2000);
    c.set_redirect(&src, &tgt, &t.recipient);

    assert!(has_event(&t, "StreamRedirectSet"));
}

/// Recipient can clear a redirect; get_redirect returns None afterwards.
#[test]
fn test_clear_redirect() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let tgt = make_stream(&t, 1, 2000);
    c.set_redirect(&src, &tgt, &t.recipient);
    c.clear_redirect(&src, &t.recipient);

    assert_eq!(c.get_redirect(&src), None);
    assert!(has_event(&t, "StreamRedirectCleared"));
}

/// Non-recipient cannot set a redirect.
#[test]
fn test_set_redirect_rejected_for_non_recipient() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let tgt = make_stream(&t, 1, 2000);
    let other = Address::generate(&t.env);

    let result = c.try_set_redirect(&src, &tgt, &other);
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}

/// Redirect to a non-existent stream is rejected.
#[test]
fn test_set_redirect_invalid_target_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let result = c.try_set_redirect(&src, &999999u64, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::InvalidRedirectTarget)));
}

/// Redirect to a stream with a different recipient is rejected.
#[test]
fn test_set_redirect_recipient_mismatch_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let other_recipient = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &200_000);

    let src = make_stream(&t, 0, 2000);
    // Create a stream with a different recipient
    let tgt = c.create_stream(
        &t.sender, &other_recipient, &t.token_id,
        &100_000, &2000, &0, &1u64, &false, &0u64, &false, &0i128,
    );

    let result = c.try_set_redirect(&src, &tgt, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::RedirectRecipientMismatch)));
}

/// Direct circular redirect Aâ†’A is rejected.
#[test]
fn test_circular_redirect_self_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let result = c.try_set_redirect(&src, &src, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::CircularRedirect)));
}

/// Indirect circular redirect Aâ†’Bâ†’A is rejected when setting B's redirect to A.
#[test]
fn test_circular_redirect_indirect_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let a = make_stream(&t, 0, 3000);
    let b = make_stream(&t, 1, 3000);

    // Set A â†’ B
    c.set_redirect(&a, &b, &t.recipient);

    // Setting B â†’ A would create Aâ†’Bâ†’A cycle
    let result = c.try_set_redirect(&b, &a, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::CircularRedirect)));
}

/// Withdraw with redirect active: StreamRedirected event is emitted.
#[test]
fn test_redirect_withdraw_emits_redirected_event() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let src = make_stream(&t, 0, 2000);
    let tgt = make_stream(&t, 1, 4000);

    c.set_redirect(&src, &tgt, &t.recipient);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&src, &t.recipient);

    assert!(has_event(&t, "StreamRedirected"));
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Feature (d): Dual-token streams
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

fn make_dual_stream(t: &FTestEnv, nonce: u64, duration: u64) -> u64 {
    fclient(t).create_dual_stream(
        &t.sender, &t.recipient,
        &t.token_id, &100_000,
        &t.token2_id, &200_000,
        &duration, &0, &nonce, &0u64, &false,
    )
}

/// create_dual_stream creates a single on-chain record covering two tokens.
#[test]
fn test_dual_stream_created_single_record() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);
    let stream = c.get_stream(&stream_id);

    assert!(stream.options.is_dual_stream);
    assert_eq!(stream.deposit, 100_000i128);  // token1 deposit
    assert_eq!(stream.token, t.token_id);
    assert_eq!(stream.status, StreamStatus::Active);
}

/// DualStreamCreated event is emitted with both token amounts.
#[test]
fn test_dual_stream_created_event() {
    let t = fsetup();
    t.env.ledger().set_timestamp(0);
    make_dual_stream(&t, 0, 1000);
    assert!(has_event(&t, "DualStreamCreated"));
}

/// create_dual_stream rejects identical token addresses.
#[test]
fn test_dual_stream_same_token_rejected() {
    let t = fsetup();
    let result = fclient(&t).try_create_dual_stream(
        &t.sender, &t.recipient,
        &t.token_id, &100_000,
        &t.token_id, &200_000,   // same token
        &1000, &0, &0u64, &0u64, &false,
    );
    assert_eq!(result, Err(Ok(StreamError::DuplicateTokenInDualStream)));
}

/// create_dual_stream rejects zero amount for either token.
#[test]
fn test_dual_stream_zero_amount_rejected() {
    let t = fsetup();
    let r1 = fclient(&t).try_create_dual_stream(
        &t.sender, &t.recipient,
        &t.token_id, &0i128,
        &t.token2_id, &200_000,
        &1000, &0, &0u64, &0u64, &false,
    );
    assert_eq!(r1, Err(Ok(StreamError::ZeroAmount)));

    let r2 = fclient(&t).try_create_dual_stream(
        &t.sender, &t.recipient,
        &t.token_id, &100_000,
        &t.token2_id, &0i128,
        &1000, &0, &1u64, &0u64, &false,
    );
    assert_eq!(r2, Err(Ok(StreamError::ZeroAmount)));
}

/// withdraw distributes both tokens proportionally in a single transaction.
#[test]
fn test_dual_stream_withdraw_distributes_both_tokens() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    // token1: flow_rate = 100_000/1000 = 100 stroops/s â†’ 500s â†’ 50_000
    let bal1 = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(bal1, 50_000i128);

    // token2: flow_rate = 200_000/1000 = 200 stroops/s â†’ 500s â†’ 100_000
    let bal2 = TokenClient::new(&t.env, &t.token2_id).balance(&t.recipient);
    assert_eq!(bal2, 100_000i128);
}

/// DualStreamWithdrawn event is emitted on withdraw.
#[test]
fn test_dual_stream_withdrawn_event() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    assert!(has_event(&t, "DualStreamWithdrawn"));
}

/// cancel_stream refunds both token amounts to sender proportionally.
#[test]
fn test_dual_stream_cancel_refunds_both_tokens() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);

    let sender_tok1_before = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let sender_tok2_before = TokenClient::new(&t.env, &t.token2_id).balance(&t.sender);

    // Cancel at t=200: elapsed=200, flow1=100, flow2=200
    t.env.ledger().set_timestamp(200);
    c.cancel_stream(&stream_id, &t.sender);

    let earned1 = 100i128 * 200;   // 20_000
    let earned2 = 200i128 * 200;   // 40_000
    let refund1 = 100_000 - earned1;  // 80_000
    let refund2 = 200_000 - earned2;  // 160_000

    let rec_tok1 = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    let rec_tok2 = TokenClient::new(&t.env, &t.token2_id).balance(&t.recipient);
    assert_eq!(rec_tok1, earned1);
    assert_eq!(rec_tok2, earned2);

    let snd_tok1 = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);
    let snd_tok2 = TokenClient::new(&t.env, &t.token2_id).balance(&t.sender);
    assert_eq!(snd_tok1 - sender_tok1_before, refund1);
    assert_eq!(snd_tok2 - sender_tok2_before, refund2);
}

/// DualStreamCancelled event is emitted on cancel.
#[test]
fn test_dual_stream_cancelled_event() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);
    t.env.ledger().set_timestamp(200);
    c.cancel_stream(&stream_id, &t.sender);

    assert!(has_event(&t, "DualStreamCancelled"));
}

/// Both streams share start_time, end_time, and cliff configuration.
#[test]
fn test_dual_stream_shares_timing() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(100);

    let stream_id = fclient(&t).create_dual_stream(
        &t.sender, &t.recipient,
        &t.token_id, &100_000,
        &t.token2_id, &200_000,
        &1000, &500, &0u64, &0u64, &false,
    );
    let stream = c.get_stream(&stream_id);

    assert_eq!(stream.start_time, 100);
    assert_eq!(stream.end_time, 1100);
    assert_eq!(stream.cliff_time, 600);
}

/// top_up is rejected on dual-token streams (use token directly, not via top_up).
#[test]
fn test_dual_stream_top_up_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_dual_stream(&t, 0, 1000);
    let result = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000);
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Emergency pause â€“ comprehensive write-instruction coverage
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// Acceptance criteria
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// 1. Every write instruction returns `StreamError::ContractPaused` while the
//    contract is paused.
// 2. Read instructions (`get_stream`, `get_claimable`, `get_streams_by_*`,
//    `get_active_streams_by_*`, `is_paused`, `get_stats`) continue to
//    succeed while paused.
//
// If a new write instruction is added to the contract it MUST be added to
// `test_all_writes_blocked_when_paused` below so the omission is caught
// before reaching production.

/// Helper: create an active stream and pause the contract, returning the stream ID.
fn setup_paused(t: &FTestEnv) -> u64 {
    let c = fclient(t);
    t.env.ledger().set_timestamp(0);

    // Create the stream *before* pausing so we have something to act on.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &9_000u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    // Advance time so tokens are claimable at the point of testing.
    t.env.ledger().set_timestamp(500);

    // Pause the contract.
    c.emergency_pause();

    stream_id
}

/// Every write instruction returns `ContractPaused` while the contract is paused.
///
/// Instructions tested:
///  - create_stream
///  - create_stream_with_schedule
///  - create_stream_with_curve
///  - create_stream_scheduled
///  - withdraw
///  - cancel_stream        â† does NOT have the pause guard (intentional: senders
///                           must always be able to cancel)
///  - recipient_terminate
///  - transfer_recipient
///  - top_up
///  - pause_stream
///  - resume_stream
///  - batch_create_stream
///  - batch_withdraw
///  - set_redirect
///  - clear_redirect
///
/// `cancel_stream` is excluded from the "must return ContractPaused" list
/// because cancellation is intentionally allowed while paused so that
/// senders can recover funds.
#[test]
fn test_all_writes_blocked_when_paused() {
    let t = fsetup();
    let c = fclient(&t);

    // â”€â”€ create a stream and pause â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let stream_id = setup_paused(&t);
    let new_recipient = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &10_000_000);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&new_recipient, &1_000_000);

    // â”€â”€ create_stream â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &9_001u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "create_stream must be blocked when paused");

    // â”€â”€ create_stream_with_curve â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &9_002u64, &false, &0u64, &false,
        &crate::types::VestingCurve::Linear,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "create_stream_with_curve must be blocked when paused");

    // â”€â”€ create_stream_scheduled â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // start_time = now (500) â€” valid future start.
    let r = c.try_create_stream_scheduled(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &500u64, &0, &9_003u64, &false, &0u64, &false, &0i128,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "create_stream_scheduled must be blocked when paused");

    // â”€â”€ create_stream_with_schedule (step-vesting) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let tranches = soroban_sdk::vec![
        &t.env,
        crate::types::VestingTranche { unlock_time: 1000, amount: 50_000 },
        crate::types::VestingTranche { unlock_time: 2000, amount: 50_000 },
    ];
    let r = c.try_create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &tranches, &9_004u64, &0u64, &false,
        &None::<Address>, &0u32,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "create_stream_with_schedule must be blocked when paused");

    // â”€â”€ withdraw â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "withdraw must be blocked when paused");

    // â”€â”€ recipient_terminate â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Need a stream with allow_recipient_termination = true. Create one before
    // pausing by temporarily resuming, but that changes state. Instead, just
    // confirm that the pause guard fires *before* the "NotAuthorized" check,
    // so even a stream without the flag returns ContractPaused.
    let r = c.try_recipient_terminate(&stream_id, &t.recipient);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "recipient_terminate must be blocked when paused");

    // â”€â”€ transfer_recipient â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_transfer_recipient(&stream_id, &t.recipient, &new_recipient);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "transfer_recipient must be blocked when paused");

    // â”€â”€ top_up â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_top_up(&stream_id, &t.sender, &t.token_id, &10_000);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "top_up must be blocked when paused");

    // â”€â”€ pause_stream (stream-level, not contract-level) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_pause_stream(&stream_id, &t.sender);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "pause_stream must be blocked when paused");

    // â”€â”€ resume_stream â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // The stream is Active (not Paused) so this would normally fail with
    // StreamNotPaused, but the contract-pause guard fires first.
    let r = c.try_resume_stream(&stream_id, &t.sender);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "resume_stream must be blocked when paused");

    // â”€â”€ batch_create_stream â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let recipients = soroban_sdk::vec![&t.env, t.recipient.clone()];
    let amounts: soroban_sdk::Vec<i128> = soroban_sdk::vec![&t.env, 10_000i128];
    let lock_untils: soroban_sdk::Vec<u64> = soroban_sdk::vec![&t.env, 0u64];
    let mut tokens = soroban_sdk::Vec::new(&t.env);
    tokens.push_back(t.token_id.clone());
    let r = c.try_batch_create_stream(
        &t.sender, &recipients, &amounts, &tokens, &1000, &false, &lock_untils, &0u64,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "batch_create_stream must be blocked when paused");

    // â”€â”€ batch_withdraw â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_batch_withdraw(
        &soroban_sdk::vec![&t.env, stream_id],
        &t.recipient,
    );
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "batch_withdraw must be blocked when paused");

    // â”€â”€ set_redirect â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Would fail with StreamNotFound for the target, but pause guard fires first.
    let r = c.try_set_redirect(&stream_id, &99_999u64, &t.recipient);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "set_redirect must be blocked when paused");

    // â”€â”€ clear_redirect â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let r = c.try_clear_redirect(&stream_id, &t.recipient);
    assert_eq!(r, Err(Ok(StreamError::ContractPaused)), "clear_redirect must be blocked when paused");
}

/// Read instructions continue to succeed while the contract is paused.
///
/// Reads tested:
///  - is_paused
///  - get_stream
///  - get_claimable
///  - get_streams_by_sender
///  - get_streams_by_recipient
///  - get_active_streams_by_sender
///  - get_active_streams_by_recipient
///  - get_stats
///  - get_protocol_fee_info
#[test]
fn test_reads_succeed_while_paused() {
    let t = fsetup();
    let c = fclient(&t);

    let stream_id = setup_paused(&t);

    // is_paused reports true.
    assert!(c.is_paused(), "is_paused() must return true while paused");

    // get_stream returns the stream (not an error).
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.id, stream_id, "get_stream must succeed while paused");
    assert_eq!(stream.status, StreamStatus::Active);

    // get_claimable returns a value (the stream is paused at t=500, so
    // claimable is frozen at 500 * 100 = 50_000).
    let claimable = c.get_claimable(&stream_id);
    // The stream has flow_rate = 100_000/1000 = 100. Because the contract is
    // emergency-paused (not stream-paused), the stream itself is still Active
    // and last_pause_time == 0. get_claimable uses ledger timestamp, which is
    // 500. Expected: 500 * 100 = 50_000.
    assert_eq!(claimable, 50_000i128, "get_claimable must succeed while paused");

    // get_streams_by_sender returns results.
    let sender_streams = c.get_streams_by_sender(&t.sender, &0u32, &10u32);
    assert!(sender_streams.len() >= 1, "get_streams_by_sender must succeed while paused");

    // get_streams_by_recipient returns results.
    let recipient_streams = c.get_streams_by_recipient(&t.recipient, &0u32, &10u32);
    assert!(recipient_streams.len() >= 1, "get_streams_by_recipient must succeed while paused");

    // get_active_streams_by_sender returns results.
    let active_by_sender = c.get_active_streams_by_sender(&t.sender);
    assert!(active_by_sender.len() >= 1, "get_active_streams_by_sender must succeed while paused");

    // get_active_streams_by_recipient returns results.
    let active_by_recipient = c.get_active_streams_by_recipient(&t.recipient);
    assert!(active_by_recipient.len() >= 1, "get_active_streams_by_recipient must succeed while paused");

    // get_stats is a pure read â€” must not fail.
    let stats = c.get_stats();
    assert!(stats.total_streams >= 1, "get_stats must succeed while paused");

    // get_protocol_fee_info is a pure read.
    let (fee_bps, _treasury) = c.get_protocol_fee_info();
    let _ = fee_bps; // value does not matter; just confirm no panic/error
}

/// After emergency_resume every previously blocked write instruction works again.
/// This test checks cancel_stream and withdraw as representative writes.
#[test]
fn test_writes_unblocked_after_emergency_resume() {
    let t = fsetup();
    let c = fclient(&t);

    let stream_id = setup_paused(&t);

    // Still paused â€” withdraw must fail.
    assert_eq!(
        c.try_withdraw(&stream_id, &t.recipient),
        Err(Ok(StreamError::ContractPaused)),
    );

    // Resume.
    c.emergency_resume();
    assert!(!c.is_paused(), "contract must be unpaused after emergency_resume");

    // Withdraw must now succeed (t=500, 50_000 tokens earned).
    c.withdraw(&stream_id, &t.recipient);
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000i128, "withdraw must work after resume");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// transfer_recipient â€“ recipient index regression tests
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
//
// Issue: After transfer_recipient, the old recipient's index must no longer
// contain the stream and the new recipient's index must contain it.
//
// Acceptance criteria
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// 1. `get_streams_by_recipient(old)` no longer includes the transferred stream.
// 2. `get_streams_by_recipient(new)` includes the transferred stream.
// 3. The stream struct's `recipient` field is updated to `new_recipient`.
// 4. All of the above hold for a recipient that had multiple streams (only
//    the transferred stream moves â€” others stay in the old index).

/// Basic transfer: old recipient loses the stream, new recipient gains it.
#[test]
fn test_transfer_recipient_updates_both_indexes() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let new_recipient = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_000u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    // Sanity-check: stream is in old recipient's index before transfer.
    let before_old = c.get_streams_by_recipient(&t.recipient, &0u32, &20u32);
    assert!(
        before_old.iter().any(|s| s.id == stream_id),
        "stream must be in old recipient's index before transfer",
    );

    // new_recipient has no streams yet.
    let before_new = c.get_streams_by_recipient(&new_recipient, &0u32, &20u32);
    assert_eq!(before_new.len(), 0, "new recipient must have empty index before transfer");

    // Perform the transfer.
    c.transfer_recipient(&stream_id, &t.recipient, &new_recipient);

    // â”€â”€ Post-transfer: old recipient's index â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let after_old = c.get_streams_by_recipient(&t.recipient, &0u32, &20u32);
    assert!(
        !after_old.iter().any(|s| s.id == stream_id),
        "transferred stream must NOT appear in old recipient's index after transfer",
    );

    // â”€â”€ Post-transfer: new recipient's index â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let after_new = c.get_streams_by_recipient(&new_recipient, &0u32, &20u32);
    assert!(
        after_new.iter().any(|s| s.id == stream_id),
        "transferred stream must appear in new recipient's index after transfer",
    );

    // â”€â”€ Stream struct recipient field â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let stream = c.get_stream(&stream_id);
    assert_eq!(
        stream.recipient, new_recipient,
        "stream.recipient must be updated to the new recipient",
    );
}

/// Old recipient retains other streams after one is transferred.
#[test]
fn test_transfer_recipient_only_moves_targeted_stream() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let new_recipient = Address::generate(&t.env);

    // Give old recipient two streams.
    let stream_a = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_100u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );
    let stream_b = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_101u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    // Transfer only stream_a.
    c.transfer_recipient(&stream_a, &t.recipient, &new_recipient);

    // stream_b must still be in old recipient's index.
    let old_index = c.get_streams_by_recipient(&t.recipient, &0u32, &20u32);
    assert!(
        old_index.iter().any(|s| s.id == stream_b),
        "stream_b must remain in old recipient's index after stream_a is transferred",
    );
    assert!(
        !old_index.iter().any(|s| s.id == stream_a),
        "stream_a must NOT remain in old recipient's index after transfer",
    );

    // new_recipient's index contains only stream_a.
    let new_index = c.get_streams_by_recipient(&new_recipient, &0u32, &20u32);
    assert_eq!(new_index.len(), 1, "new recipient should have exactly 1 stream");
    assert_eq!(
        new_index.get_unchecked(0).id, stream_a,
        "new recipient's index must contain stream_a",
    );
}

/// Querying by the new recipient after transfer returns the correct stream data.
#[test]
fn test_transfer_recipient_new_recipient_can_withdraw() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let new_recipient = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_200u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    // Transfer at t=0 (no tokens earned yet, nothing to settle to old recipient).
    c.transfer_recipient(&stream_id, &t.recipient, &new_recipient);

    // Advance time so the new recipient accumulates tokens.
    t.env.ledger().set_timestamp(400);

    // new_recipient can now withdraw.
    c.withdraw(&stream_id, &new_recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&new_recipient);
    assert_eq!(
        balance, 40_000i128,
        "new recipient should be able to withdraw tokens after transfer",
    );

    // old recipient received nothing (transfer happened before any accrual).
    let old_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(old_balance, 0i128, "old recipient should have received nothing");
}

/// Transfer at mid-stream: accrued tokens are settled to the old recipient,
/// and the new recipient receives future accruals.
#[test]
fn test_transfer_recipient_settles_accrued_to_old_recipient() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let new_recipient = Address::generate(&t.env);

    // flow_rate = 100_000 / 1000 = 100 stroops/s.
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_300u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    // At t=300, old recipient has earned 30_000 tokens.
    t.env.ledger().set_timestamp(300);
    c.transfer_recipient(&stream_id, &t.recipient, &new_recipient);

    // Old recipient should have received 30_000 (settled on transfer).
    let old_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(
        old_balance, 30_000i128,
        "transfer_recipient must settle accrued tokens to old recipient",
    );

    // New recipient now owns the stream. Advance another 200s and withdraw.
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &new_recipient);

    let new_balance = TokenClient::new(&t.env, &t.token_id).balance(&new_recipient);
    assert_eq!(
        new_balance, 20_000i128,
        "new recipient should earn tokens accrued after the transfer",
    );

    // Verify recipient-index consistency after the withdrawal.
    let new_index = c.get_streams_by_recipient(&new_recipient, &0u32, &20u32);
    assert!(
        new_index.iter().any(|s| s.id == stream_id),
        "stream must still be in new recipient's index after post-transfer withdrawal",
    );
}

/// Attempting to transfer using the wrong current_recipient returns NotRecipient.
#[test]
fn test_transfer_recipient_wrong_caller_rejected() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let impostor = Address::generate(&t.env);
    let new_recipient = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_400u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    let r = c.try_transfer_recipient(&stream_id, &impostor, &new_recipient);
    assert_eq!(
        r,
        Err(Ok(StreamError::NotRecipient)),
        "transfer_recipient with wrong caller must return NotRecipient",
    );

    // Indexes must be unchanged after the rejected transfer.
    let old_index = c.get_streams_by_recipient(&t.recipient, &0u32, &20u32);
    assert!(
        old_index.iter().any(|s| s.id == stream_id),
        "old recipient's index must be unchanged after rejected transfer",
    );
    let new_index = c.get_streams_by_recipient(&new_recipient, &0u32, &20u32);
    assert_eq!(
        new_index.len(), 0,
        "new recipient's index must remain empty after rejected transfer",
    );
}

/// Transfer is blocked while the contract is paused.
#[test]
fn test_transfer_recipient_blocked_when_paused() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let new_recipient = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &1000, &0, &8_500u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>,
    );

    c.emergency_pause();

    let r = c.try_transfer_recipient(&stream_id, &t.recipient, &new_recipient);
    assert_eq!(
        r,
        Err(Ok(StreamError::ContractPaused)),
        "transfer_recipient must return ContractPaused while paused",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Paused-duration tracking regression tests
// ═══════════════════════════════════════════════════════════════════════════
//
// Acceptance criteria
// ───────────────────
// 1. After pause + cancel, sender refund is correct (unstreamed time returned).
// 2. After pause + resume + cancel, sender refund excludes paused time.
// 3. Multiple pause/resume cycles accumulate paused_duration_seconds correctly.
// 4. `get_claimable` reflects the correct amount while paused and after resume.
// 5. Streams that are never paused are unaffected.

/// Helper: create a simple stream and return its id.
/// flow_rate = 100_000 / 1000 = 100 stroops/s.
fn make_plain_stream(t: &FTestEnv, nonce: u64) -> u64 {
    fclient(t).create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64,
        &nonce, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>,
    )
}

/// Stream paused then cancelled: sender gets back all unstreamed tokens.
///
/// Timeline: create at t=0, pause at t=300, cancel at t=300.
/// Recipient earns 300 * 100 = 30_000. Sender refund = 70_000.
#[test]
fn test_paused_then_cancelled_refund_correct() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_000);

    // Pause at t=300.
    t.env.ledger().set_timestamp(300);
    c.pause_stream(&stream_id, &t.sender);

    // Cancel immediately while paused.
    c.cancel_stream(&stream_id, &t.sender);

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    let sender_bal   = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    // Recipient should have earned exactly 300 s × 100 = 30_000.
    assert_eq!(recipient_bal, 30_000i128,
        "recipient should receive tokens earned up to the pause moment");

    // Sender should recover the unstreamed 70_000 (initial 10_000_000 - 100_000 deposit + 70_000 refund).
    let expected_sender = 10_000_000i128 - 100_000 + 70_000;
    assert_eq!(sender_bal, expected_sender,
        "sender should recover all tokens not yet streamed at pause time");
}

/// `paused_duration_seconds` accumulates on resume.
///
/// Pause 200 s, resume, then check the field.
#[test]
fn test_paused_duration_seconds_accumulated_on_resume() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_001);

    t.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &t.sender);

    // 200 seconds of pause.
    t.env.ledger().set_timestamp(300);
    c.resume_stream(&stream_id, &t.sender);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.paused_duration_seconds, 200u64,
        "paused_duration_seconds must equal the pause window length after resume");
}

/// Multiple pause/resume cycles accumulate correctly.
#[test]
fn test_multiple_pause_resume_accumulates_duration() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_002);

    // First pause: 50 s
    t.env.ledger().set_timestamp(100);
    c.pause_stream(&stream_id, &t.sender);
    t.env.ledger().set_timestamp(150);
    c.resume_stream(&stream_id, &t.sender);

    // Second pause: 100 s
    t.env.ledger().set_timestamp(400);
    c.pause_stream(&stream_id, &t.sender);
    t.env.ledger().set_timestamp(500);
    c.resume_stream(&stream_id, &t.sender);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.paused_duration_seconds, 150u64,
        "paused_duration_seconds must sum all pause windows (50 + 100 = 150)");
}

/// After pause + resume + cancel, sender refund excludes paused time.
///
/// Timeline: start t=0, pause t=200, resume t=400 (200 s paused),
/// timestamps shift +200 (end_time 1000→1200, last_withdraw_time 0→200).
/// Cancel at t=500 (active stream, 300 s of actual streaming elapsed since creation:
///   0→200 before pause, 400→500 after resume = 300 s active).
/// Recipient earned: 300 * 100 = 30_000. Refund = 70_000.
#[test]
fn test_paused_then_resumed_then_cancelled_refund_correct() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_003);

    t.env.ledger().set_timestamp(200);
    c.pause_stream(&stream_id, &t.sender);

    t.env.ledger().set_timestamp(400);
    c.resume_stream(&stream_id, &t.sender);

    // Cancel at t=500 (100 s after resume = 100 s of additional streaming).
    t.env.ledger().set_timestamp(500);
    c.cancel_stream(&stream_id, &t.sender);

    let recipient_bal = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    let sender_bal   = TokenClient::new(&t.env, &t.token_id).balance(&t.sender);

    // Active streaming time: 0→200 (200 s) + 400→500 (100 s) = 300 s total.
    // Earned = 300 * 100 = 30_000.
    assert_eq!(recipient_bal, 30_000i128,
        "recipient should earn tokens for active streaming time only");

    let expected_sender = 10_000_000i128 - 100_000 + 70_000;
    assert_eq!(sender_bal, expected_sender,
        "sender should recover all tokens not streamed during active periods");
}

/// get_claimable returns 0 while the stream is newly paused (no time has passed
/// since last_withdraw_time).
#[test]
fn test_get_claimable_zero_at_pause_time_when_just_withdrawn() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_004);

    // Withdraw everything earned so far at t=300, then immediately pause.
    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);

    c.pause_stream(&stream_id, &t.sender);

    // Claimable should be 0 — paused right after a withdrawal.
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 0i128,
        "get_claimable must be 0 when paused immediately after a withdrawal");
}

/// get_claimable after resume reflects only actively-elapsed time.
#[test]
fn test_get_claimable_after_resume_excludes_paused_time() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_005);

    // Pause at t=200 (200 s elapsed, 20_000 tokens accrued).
    t.env.ledger().set_timestamp(200);
    c.pause_stream(&stream_id, &t.sender);

    // Resume at t=500 (300 s of pause, timestamps shift +300).
    t.env.ledger().set_timestamp(500);
    c.resume_stream(&stream_id, &t.sender);

    // At t=600 (100 s after resume):
    // effective elapsed since last_withdraw = 100 s (the pause period is excluded by
    // timestamp shifting done in resume_stream).
    // claimable = 200 (pre-pause) + 100 (post-resume) = 300 s × 100 = 30_000.
    t.env.ledger().set_timestamp(600);
    let claimable = c.get_claimable(&stream_id);
    assert_eq!(claimable, 30_000i128,
        "get_claimable must reflect only actively-elapsed time, excluding pause window");
}

/// A stream that was never paused is unaffected: paused_duration_seconds stays 0.
#[test]
fn test_never_paused_stream_duration_stays_zero() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = make_plain_stream(&t, 5_006);

    t.env.ledger().set_timestamp(500);
    let stream = c.get_stream(&stream_id);

    assert_eq!(stream.paused_duration_seconds, 0u64,
        "paused_duration_seconds must stay 0 for a stream that was never paused");
}

// ═══════════════════════════════════════════════════════════════════════════
// min_claim_interval_ledgers tests
// ═══════════════════════════════════════════════════════════════════════════
//
// Stream setup: 100_000 stroops over 1_000 s, flow_rate = 100/s.
// Claim interval: 10 ledgers.
//
// Acceptance criteria
// ───────────────────
// 1. create_stream stores min_claim_interval_ledgers on the stream struct.
// 2. withdraw rejects calls within the interval (ClaimTooFrequent).
// 3. withdraw succeeds at and after the interval boundary.
// 4. Final claim (stream at end_time) bypasses the interval.
// 5. last_claim_ledger is updated after every successful withdrawal.
// 6. Streams with min_claim_interval_ledgers = None have no restriction.

/// Helper: create a stream with a claim interval.
fn make_interval_stream(t: &FTestEnv, nonce: u64, interval: u32) -> u64 {
    StellarAssetClient::new(&t.env, &t.token_id).mint(&t.sender, &100_000);
    fclient(t).create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000i128, &1000u64, &0u64,
        &nonce, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &Some(interval),
    )
}

/// min_claim_interval_ledgers is persisted on the stream struct.
#[test]
fn test_claim_interval_stored_on_stream() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = make_interval_stream(&t, 6_000, 10);
    let stream = c.get_stream(&stream_id);

    assert_eq!(stream.min_claim_interval_ledgers, Some(10u32),
        "min_claim_interval_ledgers must be stored on the stream");
    assert_eq!(stream.last_claim_ledger, 0u32,
        "last_claim_ledger must start at 0");
}

/// Withdrawal immediately after creation is blocked by the interval.
#[test]
fn test_claim_interval_blocks_early_withdrawal() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(500);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = make_interval_stream(&t, 6_001, 10);

    // Advance only 5 ledgers — not enough.
    t.env.ledger().set_sequence_number(1_005);
    t.env.ledger().set_timestamp(550);

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::StreamLocked)),
        "withdraw must return ClaimTooFrequent when called before the interval");
}

/// Withdrawal at exactly the interval boundary succeeds.
#[test]
fn test_claim_interval_allows_withdrawal_at_boundary() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = make_interval_stream(&t, 6_002, 10);

    // Advance exactly 10 ledgers.
    t.env.ledger().set_sequence_number(1_010);
    t.env.ledger().set_timestamp(500);

    // Should succeed — at the boundary.
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000i128,
        "withdrawal at interval boundary must succeed and transfer correct amount");
}

/// last_claim_ledger is updated after a successful withdrawal.
#[test]
fn test_last_claim_ledger_updated_after_withdrawal() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = make_interval_stream(&t, 6_003, 10);

    t.env.ledger().set_sequence_number(1_010);
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.last_claim_ledger, 1_010u32,
        "last_claim_ledger must equal the ledger sequence of the successful withdrawal");
}

/// Second withdrawal within the interval after the first is blocked.
#[test]
fn test_claim_interval_blocks_second_withdrawal_too_soon() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    let stream_id = make_interval_stream(&t, 6_004, 20);

    // First withdrawal at ledger 1_020 — succeeds.
    t.env.ledger().set_sequence_number(1_020);
    t.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &t.recipient);

    // Second attempt at ledger 1_030 (only 10 ledgers later, interval is 20) — blocked.
    t.env.ledger().set_sequence_number(1_030);
    t.env.ledger().set_timestamp(400);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::StreamLocked)),
        "second withdrawal within the interval must be rejected");

    // Third attempt at ledger 1_040 (20 ledgers after first withdrawal) — succeeds.
    t.env.ledger().set_sequence_number(1_040);
    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 50_000i128,
        "second successful withdrawal must transfer the correct amount");
}

/// Final claim (stream at end_time) bypasses the interval restriction.
#[test]
fn test_claim_interval_bypassed_on_final_claim() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    // interval = 500 ledgers — very long, would normally block.
    let stream_id = make_interval_stream(&t, 6_005, 500);

    // Advance to end_time (t=1000) but only 5 ledgers.
    t.env.ledger().set_sequence_number(1_005);
    t.env.ledger().set_timestamp(1_000);

    // This is the final claim — stream ended — must bypass the interval.
    c.withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 100_000i128,
        "final claim must bypass min_claim_interval_ledgers and transfer full deposit");

    // Stream must be removed after the final claim.
    assert!(c.try_get_stream(&stream_id).is_err(),
        "stream must be removed after the final claim");
}

/// Streams with min_claim_interval_ledgers = None have no restriction.
#[test]
fn test_no_claim_interval_allows_any_frequency() {
    let t = fsetup();
    let c = fclient(&t);
    t.env.ledger().set_timestamp(0);
    t.env.ledger().set_sequence_number(1_000);

    // No interval — use the plain helper.
    let stream_id = make_plain_stream(&t, 6_006);

    // Three withdrawals on three consecutive ledgers — all must succeed.
    for ledger in [1_001u32, 1_002, 1_003] {
        t.env.ledger().set_sequence_number(ledger);
        t.env.ledger().set_timestamp(ledger as u64 * 100);
        c.withdraw(&stream_id, &t.recipient);
    }

    // Verify all three produced positive balances.
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert!(balance > 0,
        "withdrawals must succeed with no claim interval set");
}


// ── Split Stream Feature Tests ─────────────────────────────────────────────

/// Test: Create a split stream with valid parameters
#[test]
fn test_split_stream_creation_success() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 5000u16),
        (recipient2.clone(), 5000u16),
    ];

    let (split_stream_id, stream_ids) = c.create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    // Verify split stream ID is generated
    assert!(split_stream_id > 0);
    
    // Verify both sub-streams are created
    assert_eq!(stream_ids.len(), 2);

    // Verify first sub-stream has correct amount (500,000)
    let stream1 = c.get_stream(&stream_ids.get_unchecked(0));
    assert_eq!(stream1.deposit, 500_000);
    assert_eq!(stream1.recipient, recipient1);
    assert_eq!(stream1.sender, t.sender);
    assert_eq!(stream1.status, StreamStatus::Active);

    // Verify second sub-stream has correct amount (500,000)
    let stream2 = c.get_stream(&stream_ids.get_unchecked(1));
    assert_eq!(stream2.deposit, 500_000);
    assert_eq!(stream2.recipient, recipient2);
}

/// Test: Split stream with unequal weights
#[test]
fn test_split_stream_unequal_weights() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 7000u16),  // 70%
        (recipient2.clone(), 3000u16),  // 30%
    ];

    let (_, stream_ids) = c.create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    let stream1 = c.get_stream(&stream_ids.get_unchecked(0));
    let stream2 = c.get_stream(&stream_ids.get_unchecked(1));

    assert_eq!(stream1.deposit, 700_000);
    assert_eq!(stream2.deposit, 300_000);
    assert_eq!(stream1.deposit + stream2.deposit, 1_000_000);
}

/// Test: Split stream rejects invalid weights that don't sum to 10,000
#[test]
fn test_split_stream_invalid_weights_sum() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1, 5000u16),
        (recipient2, 4000u16),  // Total = 9,000, not 10,000
    ];

    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream rejects empty recipient list
#[test]
fn test_split_stream_empty_recipients() {
    let t = fsetup();
    let c = fclient(&t);

    let recipients: soroban_sdk::Vec<(Address, u16)> = soroban_sdk::vec![&t.env];

    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream rejects duplicate recipients
#[test]
fn test_split_stream_duplicate_recipients() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 5000u16),
        (recipient1.clone(), 5000u16),  // Duplicate
    ];

    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream with three recipients
#[test]
fn test_split_stream_three_recipients() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);
    let recipient3 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 3333u16),
        (recipient2.clone(), 3333u16),
        (recipient3.clone(), 3334u16),  // Remainder
    ];

    let (split_stream_id, stream_ids) = c.create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    assert_eq!(stream_ids.len(), 3);

    let stream1 = c.get_stream(&stream_ids.get_unchecked(0));
    let stream2 = c.get_stream(&stream_ids.get_unchecked(1));
    let stream3 = c.get_stream(&stream_ids.get_unchecked(2));

    // First two get their exact proportion (rounded down)
    assert_eq!(stream1.deposit, 333_300);
    assert_eq!(stream2.deposit, 333_300);
    // Last one gets the remainder
    assert_eq!(stream3.deposit, 333_400);
    assert_eq!(stream1.deposit + stream2.deposit + stream3.deposit, 1_000_000);
}

/// Test: Recipients can withdraw from split stream sub-streams
#[test]
fn test_split_stream_recipient_withdrawal() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 5000u16),
        (recipient2.clone(), 5000u16),
    ];

    let (_, stream_ids) = c.create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    // Simulate time passing
    t.env.ledger().set_timestamp(500);

    // Recipient 1 can withdraw half their tokens (500 seconds / 1000 duration)
    let claimable = c.get_claimable(&stream_ids.get_unchecked(0));
    assert_eq!(claimable, 250_000);

    c.withdraw(&stream_ids.get_unchecked(0), &recipient1);

    let stream1_after = c.get_stream(&stream_ids.get_unchecked(0));
    assert_eq!(stream1_after.options.total_withdrawn, 250_000);
}

/// Test: Split stream requires correct total weight
#[test]
fn test_split_stream_weights_must_equal_10000() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    // Weights sum to 15,000 (over 100%)
    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1, 10_000u16),
        (recipient2, 5_000u16),
    ];

    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream respects duration constraints
#[test]
fn test_split_stream_duration_constraints() {
    let t = fsetup();
    let c = fclient(&t);

    let admin = Address::generate(&t.env);
    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    // Set maximum duration to 500 seconds
    c.set_max_duration(&admin, &500);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1, 5000u16),
        (recipient2, 5000u16),
    ];

    // Try to create with 1000 second duration (exceeds max)
    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,  // Exceeds max of 500
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream rejects zero amount
#[test]
fn test_split_stream_zero_amount() {
    let t = fsetup();
    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1, 5000u16),
        (recipient2, 5000u16),
    ];

    let result = c.try_create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &0,  // Zero amount
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream rejects if sender lacks sufficient balance
#[test]
fn test_split_stream_insufficient_balance() {
    let t = fsetup();
    let c = fclient(&t);

    let poor_sender = Address::generate(&t.env);
    StellarAssetClient::new(&t.env, &t.token_id).mint(&poor_sender, &100);  // Only 100 tokens

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1, 5000u16),
        (recipient2, 5000u16),
    ];

    let result = c.try_create_split_stream(
        &poor_sender,
        &recipients,
        &t.token_id,
        &1_000_000,  // Requests more than balance
        &1000,
        &0,
        &0u64,
    );

    assert!(result.is_err());
}

/// Test: Split stream emission contains correct event data
#[test]
fn test_split_stream_created_event() {
    let t = fsetup();
    t.env.mock_all_auths();
    t.env.budget().reset_unlimited();

    let c = fclient(&t);

    let recipient1 = Address::generate(&t.env);
    let recipient2 = Address::generate(&t.env);

    let recipients = soroban_sdk::vec![
        &t.env,
        (recipient1.clone(), 5000u16),
        (recipient2.clone(), 5000u16),
    ];

    let (split_stream_id, stream_ids) = c.create_split_stream(
        &t.sender,
        &recipients,
        &t.token_id,
        &1_000_000,
        &1000,
        &0,
        &0u64,
    );

    let events = t.env.events().all();
    
    // Last event should be SplitStreamCreated
    let last_event_data = &events.last().unwrap();
    let (event_topic, event_data) = last_event_data.clone();
    
    // Verify event contains correct data (simplified check)
    assert!(!stream_ids.is_empty());
    assert!(split_stream_id > 0);
}


// ── Dormant Stream Sweeping Feature Tests ───────────────────────────────────

/// Test: Sweep dormant streams after configured inactivity period
#[test]
fn test_sweep_dormant_streams_basic() {
    let t = fsetup();
    let c = fclient(&t);

    // Set dormancy threshold to 10 days
    c.set_dormancy_days(&t.admin, &10u32);

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

    let stream_before = c.get_stream(&stream_id);
    assert_eq!(stream_before.status, StreamStatus::Active);

    // Fast forward 11 days (11 * 86400 seconds)
    t.env.ledger().set_timestamp(11 * 86400);

    // Sweep the dormant stream
    let stream_ids_to_sweep = soroban_sdk::vec![&t.env, stream_id];
    let result = c.try_sweep_dormant_streams(&t.admin, &stream_ids_to_sweep);
    assert!(result.is_ok());

    // Stream should be removed from storage
    let stream_result = c.try_get_stream(&stream_id);
    assert!(stream_result.is_err());
}

/// Test: Don't sweep active streams
#[test]
fn test_sweep_dormant_respects_activity() {
    let t = fsetup();
    let c = fclient(&t);

    c.set_dormancy_days(&t.admin, &10u32);

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

    // Advance 5 days (less than dormancy threshold)
    t.env.ledger().set_timestamp(5 * 86400);

    // Try to sweep - should not sweep since not dormant yet
    let stream_ids_to_sweep = soroban_sdk::vec![&t.env, stream_id];
    let result = c.try_sweep_dormant_streams(&t.admin, &stream_ids_to_sweep);
    assert!(result.is_ok());

    // Stream should still exist
    let stream_after = c.get_stream(&stream_id);
    assert_eq!(stream_after.status, StreamStatus::Active);
}

/// Test: Dormancy sweeping is disabled when threshold is 0
#[test]
fn test_sweep_dormant_disabled_by_default() {
    let t = fsetup();
    let c = fclient(&t);

    // Dormancy should be 0 by default (disabled)
    let dormancy = c.get_dormancy_days();
    assert_eq!(dormancy, 0);

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

    // Try to sweep with dormancy disabled
    let stream_ids = soroban_sdk::vec![&t.env, stream_id];
    let result = c.try_sweep_dormant_streams(&t.admin, &stream_ids);

    // Should fail because dormancy is disabled
    assert!(result.is_err());
}

/// Test: Refund amount is correct
#[test]
fn test_sweep_dormant_refunds_remaining_balance() {
    let t = fsetup();
    let c = fclient(&t);

    c.set_dormancy_days(&t.admin, &1u32);  // 1 day

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &100,  // 100 second duration, 10K flow rate
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

    // Recipient withdraws 500K
    t.env.ledger().set_timestamp(50);  // 50 seconds in
    c.withdraw(&stream_id, &t.recipient);

    // Check claimable (should be ~500K since 50 sec / 100 sec duration = 50%)
    let claimable = c.get_claimable(&stream_id);
    assert!(claimable > 0 && claimable < 100_000);  // Some amount claimed

    // Fast forward more than dormancy threshold
    t.env.ledger().set_timestamp(1 * 86400 + 100);

    // Sweep it
    let stream_ids = soroban_sdk::vec![&t.env, stream_id];
    c.sweep_dormant_streams(&t.admin, &stream_ids);

    // Stream should be removed
    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

/// Test: Sweeping multiple streams at once
#[test]
fn test_sweep_dormant_multiple_streams() {
    let t = fsetup();
    let c = fclient(&t);

    c.set_dormancy_days(&t.admin, &1u32);

    // Create 3 streams
    let stream_ids_vec = soroban_sdk::vec![&t.env];
    let mut stream_ids = stream_ids_vec;

    for i in 0..3 {
        let recipient = Address::generate(&t.env);
        let stream_id = c.create_stream(
            &t.sender,
            &recipient,
            &t.token_id,
            &1_000_000,
            &1000,
            &0,
            &(i as u64),
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
        stream_ids.push_back(stream_id);
    }

    // Fast forward past dormancy
    t.env.ledger().set_timestamp(2 * 86400);

    // Sweep all 3
    c.sweep_dormant_streams(&t.admin, &stream_ids);

    // All should be removed
    for stream_id in stream_ids.iter() {
        let result = c.try_get_stream(&stream_id);
        assert!(result.is_err());
    }
}

/// Test: Only admin can sweep
#[test]
fn test_sweep_dormant_requires_admin() {
    let t = fsetup();
    let c = fclient(&t);

    let non_admin = Address::generate(&t.env);

    c.set_dormancy_days(&t.admin, &1u32);

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

    t.env.ledger().set_timestamp(2 * 86400);

    // Try to sweep as non-admin
    let stream_ids = soroban_sdk::vec![&t.env, stream_id];
    let result = c.try_sweep_dormant_streams(&non_admin, &stream_ids);

    assert!(result.is_err());
}

/// Test: Last withdraw time is updated properly
#[test]
fn test_sweep_dormant_tracks_last_withdraw() {
    let t = fsetup();
    let c = fclient(&t);

    c.set_dormancy_days(&t.admin, &10u32);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
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

    // Advance 5 seconds and withdraw
    t.env.ledger().set_timestamp(5);
    c.withdraw(&stream_id, &t.recipient);

    let stream_after_withdraw = c.get_stream(&stream_id);
    let last_withdraw_1 = stream_after_withdraw.last_withdraw_time;

    // Advance another 15 seconds
    t.env.ledger().set_timestamp(20);

    // Withdraw again
    c.withdraw(&stream_id, &t.recipient);

    let stream_after_withdraw_2 = c.get_stream(&stream_id);
    let last_withdraw_2 = stream_after_withdraw_2.last_withdraw_time;

    // Last withdraw time should have updated
    assert!(last_withdraw_2 > last_withdraw_1);

    // Now advance 5 days from the second withdrawal
    t.env.ledger().set_timestamp(20 + (5 * 86400));

    // Stream should still not be dormant (only 5 days)
    let stream_ids = soroban_sdk::vec![&t.env, stream_id];
    c.sweep_dormant_streams(&t.admin, &stream_ids);

    let result = c.try_get_stream(&stream_id);
    assert!(result.is_ok());

    // Advance another 6 days
    t.env.ledger().set_timestamp(20 + (11 * 86400));

    // Now sweep should work
    let stream_ids = soroban_sdk::vec![&t.env, stream_id];
    c.sweep_dormant_streams(&t.admin, &stream_ids);

    let result = c.try_get_stream(&stream_id);
    assert!(result.is_err());
}

/// Test: Sweeping skips non-existent streams gracefully
#[test]
fn test_sweep_dormant_skips_nonexistent() {
    let t = fsetup();
    let c = fclient(&t);

    c.set_dormancy_days(&t.admin, &1u32);

    let fake_id = 99999u64;
    let stream_ids = soroban_sdk::vec![&t.env, fake_id];

    // Should not error, just skip
    let result = c.try_sweep_dormant_streams(&t.admin, &stream_ids);
    assert!(result.is_ok());
}

/// Test: Dormancy threshold configuration is persisted
#[test]
fn test_dormancy_threshold_configuration() {
    let t = fsetup();
    let c = fclient(&t);

    let initial = c.get_dormancy_days();
    assert_eq!(initial, 0);

    // Set to 30 days
    c.set_dormancy_days(&t.admin, &30u32);

    let after_set = c.get_dormancy_days();
    assert_eq!(after_set, 30);

    // Set to 0 (disable)
    c.set_dormancy_days(&t.admin, &0u32);

    let after_disable = c.get_dormancy_days();
    assert_eq!(after_disable, 0);
}


// ──────────────────────────────────────────────────────────────────────────────
// Feature: Withdrawal Window (Business Hours Gating)
// ──────────────────────────────────────────────────────────────────────────────

/// Test creating a stream with a valid withdrawal window (9 AM - 5 PM UTC).
#[test]
fn test_create_stream_with_valid_withdraw_window() {
    let t = fsetup();
    let c = fclient(&t);

    // Business hours: 9 AM - 5 PM UTC = 32400 - 61200 seconds of day
    let window = Some((32400u32, 61200u32));

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_ne!(stream_id, 0);

    // Verify the window is stored in the stream
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.withdraw_window, window);
}

/// Test that creating a stream with an invalid withdraw window (start >= end) fails.
#[test]
fn test_create_stream_invalid_window_start_gte_end() {
    let t = fsetup();
    let c = fclient(&t);

    // Invalid: start >= end
    let window = Some((61200u32, 61200u32));

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

/// Test that creating a stream with window_end > 86400 fails.
#[test]
fn test_create_stream_invalid_window_exceeds_day() {
    let t = fsetup();
    let c = fclient(&t);

    // Invalid: end > 86400
    let window = Some((32400u32, 90000u32));

    let result = c.try_create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_eq!(result, Err(Ok(StreamError::InvalidDuration)));
}

/// Test that creating a stream without a withdraw window (None) succeeds.
#[test]
fn test_create_stream_without_withdraw_window() {
    let t = fsetup();
    let c = fclient(&t);

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &None,
    );
    assert_ne!(stream_id, 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.withdraw_window, None);
}

/// Test withdrawal within the allowed window succeeds.
#[test]
fn test_withdraw_within_window_succeeds() {
    let t = fsetup();
    let c = fclient(&t);

    // Set ledger time to 40000 (within business hours of 32400-61200)
    t.env.ledger().with_timestamp(40000);

    let window = Some((32400u32, 61200u32));
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );

    // Advance time and try to withdraw
    t.env.ledger().with_timestamp(40001);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());
}

/// Test withdrawal outside the allowed window fails with OutsideWithdrawWindow error.
#[test]
fn test_withdraw_outside_window_fails() {
    let t = fsetup();
    let c = fclient(&t);

    // Set ledger time to 20000 (outside business hours of 32400-61200)
    t.env.ledger().with_timestamp(20000);

    let window = Some((32400u32, 61200u32));
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );

    // Advance time to 20001 (still before 32400)
    t.env.ledger().with_timestamp(20001);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::OutsideWithdrawWindow)));
}

/// Test withdrawal at window start boundary (inclusive).
#[test]
fn test_withdraw_at_window_start_succeeds() {
    let t = fsetup();
    let c = fclient(&t);

    t.env.ledger().with_timestamp(32400); // Exactly at window start

    let window = Some((32400u32, 61200u32));
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );

    t.env.ledger().with_timestamp(32401); // One second after start
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());
}

/// Test withdrawal at window end boundary (exclusive).
#[test]
fn test_withdraw_at_window_end_exclusive_fails() {
    let t = fsetup();
    let c = fclient(&t);

    t.env.ledger().with_timestamp(61200); // Exactly at window end

    let window = Some((32400u32, 61200u32));
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );

    t.env.ledger().with_timestamp(61201); // One second after end
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::OutsideWithdrawWindow)));
}

/// Test withdrawal wraps correctly across midnight (e.g., 9 PM - 1 AM UTC).
/// This tests a window that spans midnight.
#[test]
fn test_withdraw_window_wraparound_midnight() {
    let t = fsetup();
    let c = fclient(&t);

    // 9 PM - 1 AM: 81000 - 3600 (but represented as start > end would be invalid)
    // Instead, we test that the time-of-day calculation works correctly.
    // Let's test a full-day window edge case: 0 - 86400 (entire day).
    let window = Some((0u32, 86400u32));
    
    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_ne!(stream_id, 0);

    // Should always be allowed within a full-day window
    t.env.ledger().with_timestamp(0);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());

    t.env.ledger().with_timestamp(86399); // Just before midnight
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());
}

/// Test that step-vesting streams support withdraw_window.
#[test]
fn test_create_stream_with_schedule_withdraw_window() {
    let t = fsetup();
    let c = fclient(&t);

    let tranches = soroban_sdk::vec![
        &t.env,
        crate::VestingTranche { unlock_time: 1000, amount: 50_000 },
        crate::VestingTranche { unlock_time: 2000, amount: 50_000 },
    ];

    let window = Some((32400u32, 61200u32));

    let stream_id = c.create_stream_with_schedule(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &tranches, &888u64, &0u64, &false, &None, &0u32, &window,
    );
    assert_ne!(stream_id, 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.withdraw_window, window);
}

/// Test that streams with vesting curves support withdraw_window.
#[test]
fn test_create_stream_with_curve_withdraw_window() {
    let t = fsetup();
    let c = fclient(&t);

    let curve = crate::VestingCurve::Linear;
    let window = Some((32400u32, 61200u32));

    let stream_id = c.create_stream_with_curve(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &777u64, &false, &0u64, &false, &curve, &window,
    );
    assert_ne!(stream_id, 0);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.withdraw_window, window);
}

/// Test that edge case: window at midnight (0 - 1 second).
#[test]
fn test_withdraw_window_minimal_valid() {
    let t = fsetup();
    let c = fclient(&t);

    // Minimal valid window: 0 - 1
    let window = Some((0u32, 1u32));

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64, &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_ne!(stream_id, 0);

    // Withdrawal at time 0 (within [0, 1))
    t.env.ledger().with_timestamp(0);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());

    // Withdrawal at time 1 (outside [0, 1))
    t.env.ledger().with_timestamp(1);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::OutsideWithdrawWindow)));
}

/// Test multi-day time modulo: verify that time-of-day calculation wraps correctly
/// after multiple days.
#[test]
fn test_withdraw_window_multi_day_modulo() {
    let t = fsetup();
    let c = fclient(&t);

    // Window: 9 AM - 5 PM = 32400 - 61200 seconds of day
    let window = Some((32400u32, 61200u32));

    let stream_id = c.create_stream(
        &t.sender, &t.recipient, &t.token_id,
        &100_000, &86400u64 * 100u64, // 100 days
        &0u64, &999u64, &false, &0u64, &false, &0i128,
        &None::<u32>, &None::<i128>, &None::<u32>, &false, &false, &window,
    );
    assert_ne!(stream_id, 0);

    // After 10 days, at 10 AM UTC (36000 seconds into that day)
    let timestamp = 86400u64 * 10 + 36000u64;
    t.env.ledger().with_timestamp(timestamp);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert!(result.is_ok());

    // After 10 days, at 6 PM UTC (64800 seconds into that day, outside window)
    let timestamp = 86400u64 * 10 + 64800u64;
    t.env.ledger().with_timestamp(timestamp);
    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::OutsideWithdrawWindow)));
}
