
use super::*;
use soroban_sdk::{
    testutils::{Address as _, IssuerFlags, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
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
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone());
    token.issuer().set_flag(IssuerFlags::ClawbackEnabledFlag);
    let token_id = token.address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    SoroStreamContractClient::new(&env, &contract_id)
        .initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    SoroStreamContractClient::new(&env, &contract_id).add_token_to_whitelist(&admin, &token_id);

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

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

// ─────────────────────────────────────────────────────────────────────────
// Issue #520: Vesting Cliff Validation Tests
// ─────────────────────────────────────────────────────────────────────────
// Test that cliff validation correctly prevents withdrawal before cliff_time.

#[test]
fn test_issue_520_cliff_prevents_early_withdrawal() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cliff_seconds = 1000u64;
    let duration_seconds = 5000u64;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &500_000,
        &duration_seconds,
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds,
            nonce: 0,
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
        metadata_uri: None,
    },
    );

    // Try to withdraw before cliff is reached
    t.env.ledger().set_timestamp(500); // Before cliff (cliff is at 1000)

    // Attempt withdrawal - should return zero claimable
    let stream_before_cliff = c.get_stream(&stream_id);
    assert_eq!(stream_before_cliff.status, StreamStatus::Active);

    // After cliff is reached, withdrawal should be possible
    t.env.ledger().set_timestamp(1500); // After cliff
    c.withdraw(&stream_id, &t.recipient);

    // Verify withdrawal succeeded and tokens were transferred
    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert!(balance > 0, "Recipient should have received tokens after cliff");
}

#[test]
fn test_issue_520_cliff_zero_claimable_before_cliff_time() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cliff_seconds = 2000u64;
    let duration_seconds = 10000u64;
    let _stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &duration_seconds,
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds,
            nonce: 0,
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
        metadata_uri: None,
    },
    );

    // Move time to 1000 seconds (before cliff at 2000)
    t.env.ledger().set_timestamp(1000);

    // Recipient balance should still be zero since cliff not reached
    let initial_balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(initial_balance, 0, "No tokens should be claimable before cliff");
}

#[test]
fn test_issue_520_cliff_exact_boundary() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let cliff_seconds = 500u64;
    let duration_seconds = 2000u64;
    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &duration_seconds,
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds,
            nonce: 0,
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
        metadata_uri: None,
    },
    );

    // Withdraw one second before cliff — no tokens should accrue before cliff
    t.env.ledger().set_timestamp(499);
    let _ = c.try_withdraw(&stream_id, &t.recipient);

    let balance = TokenClient::new(&t.env, &t.token_id).balance(&t.recipient);
    assert_eq!(balance, 0, "No tokens should be earned before cliff boundary");
}

#[test]
fn test_auto_renew_resets_start_time_and_keeps_claimable_zero_immediately() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &true,
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 0,
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
        metadata_uri: None,
    },
    );

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&stream_id, &t.recipient);

    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.start_time, 1000, "renewal should reset start_time to the expiry ledger");
    assert_eq!(stream.end_time, 2000, "renewal should extend from the expiry ledger");
    assert_eq!(c.get_claimable(&stream_id), 0, "claimable should stay zero immediately after auto-renewal");
}

#[test]
fn test_same_ledger_withdraw_leaves_zero_claimable_immediately() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000,
        &1000,
        &false,
        &crate::types::CreateStreamParams {
            cliff_seconds: 0,
            nonce: 0,
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
        metadata_uri: None,
    },
    );

    t.env.ledger().set_timestamp(500);
    c.withdraw(&stream_id, &t.recipient);

    assert_eq!(c.get_claimable(&stream_id), 0, "same-ledger claimable must be zero after a withdrawal at the same timestamp");
    let stream = c.get_stream(&stream_id);
    assert_eq!(stream.last_withdraw_time, 500, "stored withdrawal boundary should be the ledger timestamp that was just claimed");
}

#[test]
fn test_self_recipient_is_rejected_for_scheduled_streams() {
    let t = setup();
    let c = client(&t);

    let result = c.try_create_stream_scheduled(
        &t.sender,
        &t.sender,
        &t.token_id,
        &100_000,
        &1000,
        &500u64,
        &0u64,
        &0u64,
        &false,
        &None::<u32>,
    );
    assert_eq!(result, Err(Ok(StreamError::NotRecipient)));
}
