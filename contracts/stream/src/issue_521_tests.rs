use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String,
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
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    let admin = Address::generate(&env);
    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.initialize(&admin, &String::from_str(&env, "1.0.0"));
    c.set_min_duration(&admin, &0u64);
    // Streams can only use whitelisted tokens.
    c.add_token_to_whitelist(&admin, &token_id);

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

fn params(nonce: u64) -> crate::types::CreateStreamParams {
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
        priority: None,
        tags: None,
        metadata_uri: None,
    }
}

fn contract_balance(t: &TestEnv) -> i128 {
    TokenClient::new(&t.env, &t.token_id).balance(&t.contract_id)
}

/// Escrow still owed for streams that are known to exist.
fn owed_escrow(c: &SoroStreamContractClient<'_>, ids: &[u64]) -> i128 {
    let mut owed = 0i128;
    for id in ids {
        let stream = c.get_stream(id);
        let holds_escrow = matches!(
            stream.status,
            StreamStatus::Active
                | StreamStatus::Paused
                | StreamStatus::PendingApproval
                | StreamStatus::EscrowHold
        );
        if holds_escrow {
            owed += stream
                .deposit
                .saturating_sub(stream.options.total_withdrawn);
        }
    }
    owed
}

fn assert_stream_sane(c: &SoroStreamContractClient<'_>, id: u64) {
    let stream = c.get_stream(&id);
    assert!(stream.deposit >= 0, "deposit must be non-negative");
    assert!(
        stream.options.total_withdrawn >= 0,
        "total_withdrawn must be non-negative"
    );
    assert!(stream.flow_rate >= 0, "flow rate must be non-negative");
    assert!(
        stream.start_time <= stream.end_time,
        "start_time must not be after end_time"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Issue #521: debug-mode accounting invariant assertions
// ─────────────────────────────────────────────────────────────────────────
// These tests drive the state-changing entry points that run the assertions.
// In a debug build a violated invariant panics inside the call itself; the
// assertions below additionally verify the observable accounting
// (contract balance covers the escrow owed to live streams).

#[test]
fn test_issue_521_accounting_holds_across_lifecycle() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let s1 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1),
    );
    let s2 = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &200_000i128,
        &2000u64,
        &false,
        &params(2),
    );

    assert_stream_sane(&c, s1);
    assert_stream_sane(&c, s2);
    assert!(contract_balance(&t) >= owed_escrow(&c, &[s1, s2]));

    // Partial withdrawal, then a top-up — both run the assertions.
    t.env.ledger().set_timestamp(500);
    c.withdraw(&s1, &t.recipient);
    t.env.ledger().set_timestamp(600);
    c.top_up(&s1, &t.sender, &t.token_id, &10_000i128);

    assert_stream_sane(&c, s1);
    assert!(contract_balance(&t) >= owed_escrow(&c, &[s1, s2]));

    // Cancel the second stream; its escrow leaves the books.
    c.cancel_stream(&s2, &t.sender);

    assert_stream_sane(&c, s1);
    assert!(contract_balance(&t) >= owed_escrow(&c, &[s1]));

    // Pause / resume shift the schedule but must not break accounting.
    c.pause_stream(&s1, &t.sender, &None::<soroban_sdk::String>);
    t.env.ledger().set_timestamp(800);
    c.resume_stream(&s1, &t.sender);

    assert_stream_sane(&c, s1);
    assert!(contract_balance(&t) >= owed_escrow(&c, &[s1]));
}

#[test]
fn test_issue_521_full_withdrawal_leaves_no_escrow() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let s = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &false,
        &params(1),
    );

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&s, &t.recipient);

    // Non-renewing stream is settled and removed.
    assert!(c.try_get_stream(&s).is_err());
    assert_eq!(contract_balance(&t), 0);
    assert_eq!(
        TokenClient::new(&t.env, &t.token_id).balance(&t.recipient),
        100_000
    );
}

#[test]
fn test_issue_521_auto_renew_keeps_contract_solvent() {
    let t = setup();
    let c = client(&t);
    t.env.ledger().set_timestamp(0);

    let s = c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &100_000i128,
        &1000u64,
        &true,
        &params(1),
    );

    t.env.ledger().set_timestamp(1000);
    c.withdraw(&s, &t.recipient);

    // The stream restarts from the completion ledger with a fresh cycle.
    let stream = c.get_stream(&s);
    assert_eq!(stream.start_time, 1000);
    assert_eq!(stream.end_time, 2000);
    assert_stream_sane(&c, s);

    assert!(contract_balance(&t) >= owed_escrow(&c, &[s]));
}
