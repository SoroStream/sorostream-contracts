//! Tests for issue #622 — Recipient can be a contract address that rejects
//! transfers, locking recipient balance.
//!
//! Acceptance criteria:
//!   • At stream creation the recipient address is validated via a transfer preflight.
//!   • If the preflight fails, stream creation panics with `StreamError::InvalidRecipient`.
//!   • Creating a stream to the token contract itself (a common misconfiguration that
//!     would lock funds) is rejected with `InvalidRecipient`.
//!   • A normal EOA recipient is still accepted.

#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::errors::StreamError;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
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
    let admin = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let client = SoroStreamContractClient::new(&env, &contract_id);
    client.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    client.set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        admin,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn default_params(env: &Env, nonce: u64) -> crate::types::CreateStreamParams {
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
    }
}

// ─── Issue #622 tests ───────────────────────────────────────────────────────

/// Using the token contract itself as the recipient must be rejected.
/// Sending tokens to the token contract would lock them permanently.
#[test]
fn test_622_token_contract_as_recipient_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // The token contract address is used as the recipient — should be rejected
    // because sending tokens to the token contract itself locks them.
    let result = c.try_create_stream(
        &t.sender,
        &t.token_id,  // recipient = token contract itself
        &t.token_id,
        &1_000_000i128,
        &5_000u64,
        &false,
        &default_params(&t.env, 100),
    );

    assert_eq!(
        result.err().unwrap().unwrap(),
        StreamError::InvalidRecipient,
        "Token contract as recipient must be rejected with InvalidRecipient"
    );
}

/// A valid EOA recipient (wallet address) must still be accepted.
#[test]
fn test_622_valid_eoa_recipient_accepted() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let recipient = Address::generate(&t.env);

    let stream_id = c.create_stream(
        &t.sender,
        &recipient,
        &t.token_id,
        &1_000_000i128,
        &5_000u64,
        &false,
        &default_params(&t.env, 101),
    );

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.recipient, recipient, "Stream recipient must match");
}

/// The contract address itself must be rejected as a recipient (self-stream).
#[test]
fn test_622_self_contract_as_recipient_rejected() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    // Using the stream contract itself as recipient
    let result = c.try_create_stream(
        &t.sender,
        &t.contract_id,  // recipient = this contract
        &t.token_id,
        &1_000_000i128,
        &5_000u64,
        &false,
        &default_params(&t.env, 102),
    );

    // Should be rejected — either NotRecipient (same as contract) or InvalidRecipient
    assert!(
        result.is_err(),
        "Stream contract as recipient must be rejected"
    );
}
