//! Tests for issue #638: batch_top_up_streams entry point.
//!
//! Validates that:
//! - Batch top-up of 5 streams succeeds atomically.
//! - If any stream in the batch is invalid the entire batch is rejected.
//! - A batch with more than 20 entries is rejected.
//! - A non-sender caller is rejected.
//! - A cancelled/non-active stream causes the batch to fail.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

struct Setup {
    env: Env,
    contract: Address,
    token: Address,
    sender: Address,
    recipient: Address,
}

fn setup_638() -> Setup {
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

    // Mint plenty of tokens for all tests.
    StellarAssetClient::new(&env, &token).mint(&sender, &100_000_000);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);

    Setup { env, contract, token, sender, recipient }
}

fn make_stream(s: &Setup, amount: i128, duration: u64, nonce: u64) -> u64 {
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
            },
        )
        .unwrap()
}

fn token_balance(s: &Setup, who: &Address) -> i128 {
    TokenClient::new(&s.env, &s.token).balance(who)
}

/// Batch top-up of exactly 5 streams succeeds atomically.
/// Each stream receives an additional top-up amount, and the sender's balance
/// decreases by the total of all effective amounts.
#[test]
fn test_638_batch_top_up_5_streams_succeeds() {
    let s = setup_638();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // Create 5 streams with flow_rate = 1000/1000 = 1 token/sec.
    let ids: Vec<u64> = (1..=5u64)
        .map(|i| make_stream(&s, 1000, 1000, i))
        .collect::<std::vec::Vec<_>>()
        .into_iter()
        .fold(Vec::new(&s.env), |mut v, id| { v.push_back(id); v });

    let balance_before = token_balance(&s, &s.sender);

    // Top up each stream with 100 tokens (multiple of flow_rate=1 so effective=100).
    let mut pairs: Vec<(u64, i128)> = Vec::new(&s.env);
    for i in 0..ids.len() {
        pairs.push_back((ids.get_unchecked(i), 100i128));
    }

    c.batch_top_up_streams(&s.sender, &s.token, &pairs).unwrap();

    let balance_after = token_balance(&s, &s.sender);
    assert_eq!(
        balance_before - balance_after,
        500i128,
        "sender balance must decrease by 5 * 100 = 500"
    );

    // Verify each stream's end_time was extended by 100 / 1 = 100 seconds.
    for i in 0..ids.len() {
        let stream = c.get_stream(&ids.get_unchecked(i)).unwrap();
        // original end_time = 0 + 1000 = 1000; after top-up = 1000 + 100 = 1100.
        assert_eq!(stream.end_time, 1100u64, "each stream end_time must be extended by 100 s");
    }
}

/// If any stream in the batch is invalid (cancelled), the entire batch fails
/// and no state is changed.
#[test]
fn test_638_batch_fails_atomically_if_one_stream_invalid() {
    let s = setup_638();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let id_good1 = make_stream(&s, 1000, 1000, 10);
    let id_good2 = make_stream(&s, 1000, 1000, 11);
    let id_bad   = make_stream(&s, 1000, 1000, 12);

    // Cancel the third stream so it is no longer Active.
    c.cancel_stream(&id_bad, &s.sender).unwrap();

    let balance_before = token_balance(&s, &s.sender);

    let mut pairs: Vec<(u64, i128)> = Vec::new(&s.env);
    pairs.push_back((id_good1, 100i128));
    pairs.push_back((id_good2, 100i128));
    pairs.push_back((id_bad,   100i128)); // should cause the batch to fail

    let result = c.try_batch_top_up_streams(&s.sender, &s.token, &pairs);
    assert!(result.is_err(), "batch must be rejected when any stream is invalid");

    // Sender balance unchanged — no tokens transferred.
    assert_eq!(
        token_balance(&s, &s.sender),
        balance_before,
        "sender balance must be unchanged after failed batch"
    );

    // Good streams must not have been modified.
    let stream1 = c.get_stream(&id_good1).unwrap();
    assert_eq!(stream1.end_time, 1000u64, "stream1 end_time must be unchanged");
    let stream2 = c.get_stream(&id_good2).unwrap();
    assert_eq!(stream2.end_time, 1000u64, "stream2 end_time must be unchanged");
}

/// A batch with more than 20 entries is rejected with BatchLengthMismatch.
#[test]
fn test_638_batch_too_large_rejected() {
    let s = setup_638();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // Build a 21-entry vec of arbitrary (stream_id=0, amount=100) pairs.
    // stream_id=0 doesn't need to exist since length check comes first.
    let mut pairs: Vec<(u64, i128)> = Vec::new(&s.env);
    for _ in 0..21 {
        pairs.push_back((0u64, 100i128));
    }

    let result = c.try_batch_top_up_streams(&s.sender, &s.token, &pairs);
    assert_eq!(
        result,
        Err(crate::StreamError::BatchLengthMismatch),
        "batch of 21 must be rejected with BatchLengthMismatch"
    );
}

/// A non-sender caller is rejected even if the amounts are valid.
#[test]
fn test_638_non_sender_rejected() {
    let s = setup_638();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = make_stream(&s, 1000, 1000, 20);
    StellarAssetClient::new(&s.env, &s.token).mint(&s.recipient, &10_000);

    let mut pairs: Vec<(u64, i128)> = Vec::new(&s.env);
    pairs.push_back((stream_id, 100i128));

    // recipient is not the stream sender.
    let result = c.try_batch_top_up_streams(&s.recipient, &s.token, &pairs);
    assert!(result.is_err(), "non-sender must be rejected");
}

/// An empty batch (zero entries) succeeds trivially and transfers nothing.
#[test]
fn test_638_empty_batch_succeeds() {
    let s = setup_638();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let balance_before = token_balance(&s, &s.sender);
    let pairs: Vec<(u64, i128)> = Vec::new(&s.env);

    c.batch_top_up_streams(&s.sender, &s.token, &pairs).unwrap();

    assert_eq!(
        token_balance(&s, &s.sender),
        balance_before,
        "empty batch must not change sender balance"
    );
}
