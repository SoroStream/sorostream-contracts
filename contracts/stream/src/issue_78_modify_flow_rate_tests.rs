//! Issue #78: `modify_flow_rate` lets the sender change the flow rate of an
//! active stream after creation.
//!
//! - The balance accrued at the old rate is settled (paid to the recipient) at
//!   the modification timestamp.
//! - The new rate applies from the modification timestamp onward.
//! - `end_time` is recalculated from the remaining balance and the new rate.
//! - Only the sender may modify the rate.
//! - The recipient's cumulative earnings are continuous across the change.

#![cfg(test)]

use crate::{errors::StreamError, SoroStreamContract, SoroStreamContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, String, Symbol,
};

const DEPOSIT: i128 = 1_000_000;
const DURATION: u64 = 1_000;
const RATE: i128 = 1_000; // DEPOSIT / DURATION

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
    env.ledger().set_timestamp(0);

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

fn create(t: &TestEnv, c: &SoroStreamContractClient<'_>) -> u64 {
    c.create_stream(
        &t.sender,
        &t.recipient,
        &t.token_id,
        &DEPOSIT,
        &DURATION,
        &false,
        &params(1),
    )
}

fn recipient_balance(t: &TestEnv) -> i128 {
    TokenClient::new(&t.env, &t.token_id).balance(&t.recipient)
}

/// Total the recipient has earned so far: already withdrawn + still claimable.
fn earned(c: &SoroStreamContractClient<'_>, id: u64) -> i128 {
    c.get_stream(&id).options.total_withdrawn + c.get_claimable(&id)
}

#[test]
fn modify_rate_up_settles_old_rate_and_shortens_end_time() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);
    assert_eq!(c.get_stream(&id).flow_rate, RATE);

    t.env.ledger().set_timestamp(400);
    assert_eq!(c.get_claimable(&id), 400_000);

    let new_end = c.modify_flow_rate(&id, &t.sender, &2_000i128);

    // Accrued 400 s × 1_000 locked in and paid at the modification timestamp.
    assert_eq!(recipient_balance(&t), 400_000);
    let s = c.get_stream(&id);
    assert_eq!(s.flow_rate, 2_000);
    assert_eq!(s.options.total_withdrawn, 400_000);
    assert_eq!(s.last_withdraw_time, 400);
    // Remaining 600_000 at 2_000/s → 300 s.
    assert_eq!(new_end, 700);
    assert_eq!(s.end_time, 700);
    assert_eq!(c.get_claimable(&id), 0);

    // New rate applies from the modification timestamp onward.
    t.env.ledger().set_timestamp(500);
    assert_eq!(c.get_claimable(&id), 200_000);

    // The full deposit is claimable by the new end time.
    t.env.ledger().set_timestamp(700);
    assert_eq!(c.get_claimable(&id), 600_000);
    c.withdraw(&id, &t.recipient);
    assert_eq!(recipient_balance(&t), DEPOSIT);
}

#[test]
fn modify_rate_down_settles_old_rate_and_extends_end_time() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);

    t.env.ledger().set_timestamp(400);
    let new_end = c.modify_flow_rate(&id, &t.sender, &500i128);

    assert_eq!(recipient_balance(&t), 400_000);
    let s = c.get_stream(&id);
    assert_eq!(s.flow_rate, 500);
    // Remaining 600_000 at 500/s → 1_200 s.
    assert_eq!(new_end, 1_600);
    assert_eq!(s.end_time, 1_600);

    // At the old end time only the slower accrual has happened.
    t.env.ledger().set_timestamp(1_000);
    assert_eq!(c.get_claimable(&id), 300_000);

    t.env.ledger().set_timestamp(1_600);
    assert_eq!(c.get_claimable(&id), 600_000);
    c.withdraw(&id, &t.recipient);
    assert_eq!(recipient_balance(&t), DEPOSIT);
}

#[test]
fn claimable_is_continuous_across_rate_changes() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);

    // Rate up.
    t.env.ledger().set_timestamp(250);
    let before = earned(&c, id);
    assert_eq!(before, 250_000);
    c.modify_flow_rate(&id, &t.sender, &3_000i128);
    assert_eq!(earned(&c, id), before, "no jump at the modification instant");
    t.env.ledger().set_timestamp(251);
    assert_eq!(earned(&c, id), before + 3_000);

    // Rate down, after some accrual at the new rate.
    t.env.ledger().set_timestamp(300);
    let before = earned(&c, id);
    assert_eq!(before, 250_000 + 50 * 3_000);
    c.modify_flow_rate(&id, &t.sender, &100i128);
    assert_eq!(earned(&c, id), before, "no jump at the modification instant");
    t.env.ledger().set_timestamp(310);
    assert_eq!(earned(&c, id), before + 10 * 100);

    // Earnings never exceed the deposit and reach it exactly at the end.
    let end = c.get_stream(&id).end_time;
    t.env.ledger().set_timestamp(end);
    assert_eq!(earned(&c, id), DEPOSIT);
}

#[test]
fn end_time_rounds_up_so_remaining_balance_is_fully_claimable() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);

    t.env.ledger().set_timestamp(400);
    // Remaining 600_000 / 7_000 = 85.71… → 86 s.
    let new_end = c.modify_flow_rate(&id, &t.sender, &7_000i128);
    assert_eq!(new_end, 486);

    t.env.ledger().set_timestamp(485);
    assert_eq!(c.get_claimable(&id), 85 * 7_000);

    t.env.ledger().set_timestamp(486);
    assert_eq!(c.get_claimable(&id), 600_000);
    c.withdraw(&id, &t.recipient);
    assert_eq!(recipient_balance(&t), DEPOSIT);
}

#[test]
fn only_sender_can_modify_flow_rate() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);
    t.env.ledger().set_timestamp(100);

    let stranger = Address::generate(&t.env);
    assert_eq!(
        c.try_modify_flow_rate(&id, &t.recipient, &2_000i128),
        Err(Ok(StreamError::NotSender))
    );
    assert_eq!(
        c.try_modify_flow_rate(&id, &stranger, &2_000i128),
        Err(Ok(StreamError::NotSender))
    );

    // Nothing changed.
    let s = c.get_stream(&id);
    assert_eq!(s.flow_rate, RATE);
    assert_eq!(s.end_time, DURATION);
    assert_eq!(recipient_balance(&t), 0);
}

#[test]
fn modify_flow_rate_rejects_invalid_input_and_state() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);
    t.env.ledger().set_timestamp(100);

    assert_eq!(
        c.try_modify_flow_rate(&id, &t.sender, &0i128),
        Err(Ok(StreamError::ZeroFlowRate))
    );
    assert_eq!(
        c.try_modify_flow_rate(&id, &t.sender, &-5i128),
        Err(Ok(StreamError::ZeroFlowRate))
    );
    assert_eq!(
        c.try_modify_flow_rate(&999u64, &t.sender, &2_000i128),
        Err(Ok(StreamError::StreamNotFound))
    );

    // Once the stream has fully accrued there is nothing left to re-rate.
    t.env.ledger().set_timestamp(DURATION);
    assert_eq!(
        c.try_modify_flow_rate(&id, &t.sender, &2_000i128),
        Err(Ok(StreamError::StreamNotActive))
    );
}

#[test]
fn modify_flow_rate_emits_event() {
    let t = setup();
    let c = client(&t);
    let id = create(&t, &c);

    t.env.ledger().set_timestamp(400);
    c.modify_flow_rate(&id, &t.sender, &2_000i128);

    let name = Symbol::new(&t.env, "FlowRateModified");
    let mut found = false;
    for (addr, topics, data) in t.env.events().all().iter() {
        if addr != t.contract_id {
            continue;
        }
        let Some(first) = topics.get(0) else { continue };
        let sym: Symbol = first.into_val(&t.env);
        if sym != name {
            continue;
        }
        let stream_id: u64 = topics.get(1).unwrap().into_val(&t.env);
        assert_eq!(stream_id, id);
        let (old_rate, new_rate, settled, new_end, ts, _nonce): (i128, i128, i128, u64, u64, u64) =
            data.into_val(&t.env);
        assert_eq!((old_rate, new_rate, settled, new_end, ts), (RATE, 2_000, 400_000, 700, 400));
        found = true;
    }
    assert!(found, "FlowRateModified event not emitted");
}
