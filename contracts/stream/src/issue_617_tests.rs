
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
    admin: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

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

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&admin, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        admin,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

fn default_params() -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
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
        tags: None,
        metadata_uri: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #617: emergency_pause requires a 1-ledger activation delay
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_issue_617_pause_is_not_active_in_the_same_ledger_it_is_requested() {
    let t = setup();
    let c = client(&t);

    c.emergency_pause();

    // Still in the same ledger the pause was requested — not yet active.
    assert!(!c.is_paused());
}

#[test]
fn test_issue_617_pause_takes_exactly_one_ledger_to_activate() {
    let t = setup();
    let c = client(&t);

    let starting_ledger = t.env.ledger().sequence();
    c.emergency_pause();
    assert!(!c.is_paused());

    // One ledger later, the pause is active.
    t.env.ledger().with_mut(|li| li.sequence_number = starting_ledger + 1);
    assert!(c.is_paused());
}

#[test]
fn test_issue_617_users_can_still_withdraw_during_the_activation_delay() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(),
    );

    // Let some of the stream vest.
    t.env.ledger().with_mut(|li| li.timestamp = 500);

    c.emergency_pause();
    // Same-ledger withdrawal must still succeed — the pause has not activated yet.
    c.withdraw(&stream_id, &t.recipient);

    let token_client = TokenClient::new(&t.env, &t.token_id);
    assert!(token_client.balance(&t.recipient) > 0);
}

#[test]
fn test_issue_617_withdraw_is_blocked_once_pause_activates() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let stream_id = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &1_000_000,
        &1000u64,
        &false,
        &default_params(),
    );
    t.env.ledger().with_mut(|li| li.timestamp = 500);

    let starting_ledger = t.env.ledger().sequence();
    c.emergency_pause();
    t.env.ledger().with_mut(|li| li.sequence_number = starting_ledger + 1);

    let result = c.try_withdraw(&stream_id, &t.recipient);
    assert_eq!(result, Err(Ok(StreamError::ContractPaused)));
}

#[test]
fn test_issue_617_resume_is_immediate_not_delayed() {
    let t = setup();
    let c = client(&t);

    let starting_ledger = t.env.ledger().sequence();
    c.emergency_pause();
    t.env.ledger().with_mut(|li| li.sequence_number = starting_ledger + 1);
    assert!(c.is_paused());

    c.emergency_resume();
    // Resuming lifts the pause right away — only activation is delayed, not lifting.
    assert!(!c.is_paused());
}
