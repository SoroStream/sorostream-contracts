//! Tests for issue #643: `partial_withdraw` entry point.
//!
//! Validates that:
//! - A recipient can withdraw less than the full claimable amount.
//! - The remaining balance continues to accrue after a partial withdrawal.
//! - The function rejects zero amounts, non-recipients, and over-claims.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_643() -> Setup {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.ledger().set_timestamp(0);

    let contract = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&sender, &100_000_000);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);

    Setup { env, contract, token, sender, recipient }
}

fn create_stream_643(s: &Setup, amount: i128, duration: u64, nonce: u64) -> u64 {
    SoroStreamContractClient::new(&s.env, &s.contract)
        .create_stream(
            &s.sender,
            &s.recipient,
            &s.token,
            &amount,
            &duration,
            &false,
            &0,
            &CreateStreamParams {
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
                tags: None,
            },
        )
        .unwrap()
}

/// A partial withdrawal of half the claimable amount succeeds and leaves the
/// other half still accruing in the stream.
#[test]
fn test_643_partial_withdrawal_leaves_remaining_balance() {
    let s = setup_643();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // 1000-stroop stream over 1000 seconds → flow_rate = 1 stroop/s.
    let stream_id = create_stream_643(&s, 1000, 1000, 1);

    // Advance to t=500 so 500 stroops are claimable.
    s.env.ledger().set_timestamp(500);

    let balance_before = TokenClient::new(&s.env, &s.token).balance(&s.recipient);

    // Withdraw only 200 of the 500 claimable stroops.
    c.partial_withdraw(&stream_id, &200i128, &s.recipient).unwrap();

    let balance_after = TokenClient::new(&s.env, &s.token).balance(&s.recipient);
    assert_eq!(balance_after - balance_before, 200, "recipient should receive exactly 200 stroops");

    // Advance time further so more accrues.
    s.env.ledger().set_timestamp(800);

    // The recipient should now be able to claim the remaining 300 from the
    // first window + 300 from the new window = 600 stroops.
    let claimable = c.get_claimable(&stream_id).unwrap();
    // At t=800, total accrued = 800.  Already withdrawn = 200.  Remaining = 600.
    assert_eq!(claimable, 600, "remaining balance must still accrue after partial withdrawal");
}

/// partial_withdraw with amount > claimable returns AmountBelowMinimum.
#[test]
fn test_643_amount_exceeds_claimable_is_rejected() {
    let s = setup_643();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = create_stream_643(&s, 1000, 1000, 2);
    s.env.ledger().set_timestamp(100); // 100 stroops claimable

    let result = c.try_partial_withdraw(&stream_id, &500i128, &s.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::AmountBelowMinimum)),
        "withdrawing more than claimable must return AmountBelowMinimum"
    );
}

/// partial_withdraw with amount == 0 returns ZeroAmount.
#[test]
fn test_643_zero_amount_rejected() {
    let s = setup_643();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = create_stream_643(&s, 1000, 1000, 3);
    s.env.ledger().set_timestamp(500);

    let result = c.try_partial_withdraw(&stream_id, &0i128, &s.recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::ZeroAmount)),
        "zero amount must be rejected"
    );
}

/// partial_withdraw called by an unrelated address returns NotAuthorized.
#[test]
fn test_643_non_recipient_rejected() {
    let s = setup_643();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = create_stream_643(&s, 1000, 1000, 4);
    s.env.ledger().set_timestamp(500);

    let attacker = Address::generate(&s.env);
    let result = c.try_partial_withdraw(&stream_id, &100i128, &attacker);
    assert_eq!(
        result,
        Err(Ok(StreamError::NotAuthorized)),
        "non-recipient must be rejected"
    );
}

#[test]
fn test_643_delegate_can_partially_withdraw_for_recipient() {
    let s = setup_643();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);
    let stream_id = create_stream_643(&s, 1000, 1000, 5);
    let delegate = Address::generate(&s.env);
    c.set_delegate(&s.sender, &stream_id, &delegate);
    s.env.ledger().set_timestamp(500);

    c.partial_withdraw(&stream_id, &200i128, &delegate);

    assert_eq!(TokenClient::new(&s.env, &s.token).balance(&s.recipient), 200);
    assert_eq!(c.get_claimable(&stream_id).unwrap(), 300);
}
