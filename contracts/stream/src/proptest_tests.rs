#![cfg(test)]

extern crate std;

use crate::{SoroStreamContract, SoroStreamContractClient};
use crate::types::StreamStatus;
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

fn setup_env() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000_000);

    // Disable minimum duration for tests
    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

    (env, contract_id, token_id, sender, recipient)
}

// ── create_stream properties ────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Balance conservation: sender loses exactly `amount`, contract gains it.
    #[test]
    fn prop_create_balance_conservation(
        amount in 100_i128..=1_000_000_i128,
        duration in 10_u64..=100_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let sender_before = token.balance(&sender);
        let contract_before = token.balance(&contract_id);

        let cliff = 0u64;
        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        c.create_stream(&sender, &recipient, &token_id, &amount, &duration, &cliff, &0u64, &false, &0u64,
        &false);
        c.create_stream(&sender, &recipient, &token_id, &amount, &duration, &cliff, &0u64, &false, &0u64, &Bytes::new(&env));

        let sender_after = token.balance(&sender);
        let contract_after = token.balance(&contract_id);

        prop_assert_eq!(sender_before - sender_after, amount);
        prop_assert_eq!(contract_after - contract_before, amount);
    }

    /// Stream fields match input parameters.
    #[test]
    fn prop_create_fields_match(
        amount in 1000_i128..=1_000_000_i128,
        duration in 10_u64..=100_000_u64,
        cliff in 0_u64..=100_000_u64,
    ) {
        let cliff = cliff.min(duration);
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(1000);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &cliff, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );

        let stream = c.get_stream(&stream_id);
        prop_assert_eq!(stream.deposit, amount);
        prop_assert_eq!(stream.flow_rate, flow_rate);
        prop_assert_eq!(stream.status, StreamStatus::Active);
        prop_assert_eq!(stream.start_time, 1000);
        prop_assert_eq!(stream.end_time, 1000 + duration);
        prop_assert_eq!(stream.cliff_time, 1000 + cliff);
    }
}

// ── withdraw properties ─────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Monotonic withdrawal: recipient balance only increases.
    #[test]
    fn prop_withdraw_monotonic(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        t1 in 1_u64..=5_000_u64,
        t2_offset in 1_u64..=5_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &0u64, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );
        let token = TokenClient::new(&env, &token_id);

        let t1 = t1.min(duration);
        env.ledger().set_timestamp(t1);
        c.withdraw(&stream_id, &recipient);
        let bal1 = token.balance(&recipient);

        let t2 = t1.saturating_add(t2_offset).min(duration);
        if t2 <= t1 { return Ok(()); }
        env.ledger().set_timestamp(t2);

        if t2 >= duration {
            // Stream completed on first withdraw if t1 >= duration, or completes now
            if c.try_get_stream(&stream_id).is_err() {
                // Stream was already removed (completed), balance can only stay same
                let bal2 = token.balance(&recipient);
                prop_assert!(bal2 >= bal1);
                return Ok(());
            }
        }

        c.withdraw(&stream_id, &recipient);
        let bal2 = token.balance(&recipient);

        prop_assert!(bal2 >= bal1, "recipient balance must be non-decreasing");
    }

    /// Withdrawal never exceeds deposit.
    #[test]
    fn prop_withdraw_bounded_by_deposit(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        withdraw_time in 0_u64..=20_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &0u64, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );
        let token = TokenClient::new(&env, &token_id);

        env.ledger().set_timestamp(withdraw_time);
        c.withdraw(&stream_id, &recipient);

        let recipient_bal = token.balance(&recipient);
        prop_assert!(recipient_bal <= amount, "withdrawn must not exceed deposit");
    }
}

// ── top_up properties ───────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Top-up increases deposit and extends end_time proportionally.
    #[test]
    fn prop_topup_extends_correctly(
        amount in 10_000_i128..=500_000_i128,
        duration in 100_u64..=10_000_u64,
        topup in 1_000_i128..=500_000_i128,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &0u64, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );
        let stream_before = c.get_stream(&stream_id);

        let effective_topup = topup - (topup % flow_rate);
        if effective_topup <= 0 { return Ok(()); }

        c.top_up(&stream_id, &sender, &token_id, &topup);

        let stream_after = c.get_stream(&stream_id);
        let extra_seconds = (effective_topup / flow_rate) as u64;

        prop_assert_eq!(stream_after.deposit, stream_before.deposit + effective_topup);
        prop_assert_eq!(stream_after.end_time, stream_before.end_time + extra_seconds);
        prop_assert_eq!(stream_after.status, StreamStatus::Active);
    }
}

// ── cancel properties ───────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Balance conservation on cancel: recipient + sender = original total.
    #[test]
    fn prop_cancel_balance_conservation(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        cancel_time in 1_u64..=10_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let sender_before = token.balance(&sender);

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &0u64, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );

        let cancel_time = cancel_time.min(duration - 1).max(1);
        env.ledger().set_timestamp(cancel_time);
        c.cancel_stream(&stream_id, &sender);

        let sender_after = token.balance(&sender);
        let recipient_after = token.balance(&recipient);

        let sender_net_loss = sender_before - sender_after;
        prop_assert_eq!(
            sender_net_loss + recipient_after, amount,
            "tokens must be fully conserved on cancel"
        );
    }

    /// Cancel sets status to Cancelled.
    #[test]
    fn prop_cancel_sets_status(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &0u64, &0u64, &false, &0u64,
        &false,
            &Bytes::new(&env),
        );

        env.ledger().set_timestamp(1);
        c.cancel_stream(&stream_id, &sender);

        let stream = c.get_stream(&stream_id);
        prop_assert_eq!(stream.status, StreamStatus::Cancelled);
    }
}

// ── pause/resume state machine properties ───────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// State machine: pause → is_paused, unpause → !is_paused, create blocked when paused.
    #[test]
    fn prop_pause_resume_state_machine(
        do_pause in proptest::bool::ANY,
        do_unpause in proptest::bool::ANY,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));

        prop_assert!(!c.is_paused());

        if do_pause {
            c.pause();
            prop_assert!(c.is_paused());

            // create_stream must fail when paused
            let result = c.try_create_stream(
                &sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
            );
            prop_assert!(result.is_err());

            if do_unpause {
                c.unpause();
                prop_assert!(!c.is_paused());

                // create_stream must work after unpause
                let result = c.try_create_stream(
                    &sender, &recipient, &token_id, &100_000, &1000, &0, &0u64, &false, &0u64,
        &false,
                );
                prop_assert!(result.is_ok());
            }
        }
    }
}
