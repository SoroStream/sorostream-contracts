//! Tests for issue #640: `get_withdrawal_proof` entry point.
//!
//! Validates that:
//! - A proof can be retrieved after a successful withdrawal.
//! - The Merkle proof verifies against the stream's current withdrawal root.
//! - Requesting a proof for a non-existent index returns `InvalidParameter`.
//! - Requesting a proof for a non-existent stream returns `StreamNotFound`.

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

fn setup_640() -> Setup {
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

    StellarAssetClient::new(&env, &token).mint(&sender, &10_000_000);

    let c = SoroStreamContractClient::new(&env, &contract);
    c.set_min_duration(&sender, &0u64);

    Setup { env, contract, token, sender, recipient }
}

fn create_stream_640(s: &Setup, amount: i128, duration: u64, nonce: u64) -> u64 {
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

/// After one withdrawal, `get_withdrawal_proof(stream_id, 0)` must return a
/// proof whose fields match and whose Merkle path verifies against the current root.
#[test]
fn test_640_proof_for_first_withdrawal() {
    let s = setup_640();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    // Create a 1000-stroop stream running for 1000 seconds (flow_rate = 1/s).
    let stream_id = create_stream_640(&s, 1000, 1000, 1);

    // Advance time to halfway point so there is something to withdraw.
    s.env.ledger().set_timestamp(500);

    c.withdraw(&stream_id, &s.recipient).unwrap();

    // Retrieve proof for the first (index 0) withdrawal.
    let proof = c.get_withdrawal_proof(&stream_id, &0u32).unwrap();

    assert_eq!(proof.stream_id, stream_id, "proof stream_id must match");
    assert_eq!(proof.withdrawal_index, 0, "proof index must be 0");
    assert!(proof.amount > 0, "proof amount must be positive");
    assert_eq!(proof.timestamp, 500, "proof timestamp must equal ledger time of withdrawal");

    // The leaf hash must be a 32-byte non-zero value.
    let hash_bytes = proof.leaf_hash.to_array();
    assert!(
        hash_bytes.iter().any(|&b| b != 0),
        "leaf_hash must not be all-zeros"
    );
    let root = c.get_withdrawal_history_root(&stream_id).unwrap();
    assert_eq!(proof.root, root);
    assert!(c.verify_withdrawal_proof(&proof));

    let mut tampered = proof.clone();
    tampered.amount += 1;
    assert!(!c.verify_withdrawal_proof(&tampered));
}

/// Requesting index 0 on a stream that has never had a withdrawal returns
/// `InvalidParameter`.
#[test]
fn test_640_no_withdrawal_returns_invalid_parameter() {
    let s = setup_640();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = create_stream_640(&s, 1000, 1000, 2);

    let result = c.try_get_withdrawal_proof(&stream_id, &0u32);
    assert_eq!(
        result,
        Err(Ok(StreamError::InvalidParameter)),
        "proof for a stream with no withdrawals must return InvalidParameter"
    );
}

/// Requesting a proof for a non-existent stream returns `StreamNotFound`.
#[test]
fn test_640_nonexistent_stream_returns_not_found() {
    let s = setup_640();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let result = c.try_get_withdrawal_proof(&999_999u64, &0u32);
    assert_eq!(
        result,
        Err(Ok(StreamError::StreamNotFound)),
        "proof for a non-existent stream must return StreamNotFound"
    );
}

/// After two withdrawals the second proof (index 1) must reflect the second withdrawal.
#[test]
fn test_640_second_withdrawal_proof() {
    let s = setup_640();
    let c = SoroStreamContractClient::new(&s.env, &s.contract);

    let stream_id = create_stream_640(&s, 1000, 1000, 3);

    // First withdrawal at t=300.
    s.env.ledger().set_timestamp(300);
    c.withdraw(&stream_id, &s.recipient).unwrap();

    // Second withdrawal at t=700.
    s.env.ledger().set_timestamp(700);
    c.withdraw(&stream_id, &s.recipient).unwrap();

    let proof0 = c.get_withdrawal_proof(&stream_id, &0u32).unwrap();
    let proof1 = c.get_withdrawal_proof(&stream_id, &1u32).unwrap();

    assert_eq!(proof0.timestamp, 300, "first proof must have timestamp 300");
    assert_eq!(proof1.timestamp, 700, "second proof must have timestamp 700");
    assert_eq!(proof1.withdrawal_index, 1, "second proof must have index 1");
    assert!(
        proof1.amount > 0,
        "second proof amount must be positive"
    );
    assert!(c.verify_withdrawal_proof(&proof0));
    assert!(c.verify_withdrawal_proof(&proof1));
}
