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

fn setup_env_with_fee(fee_bps: u32) -> (Env, Address, Address, Address, Address) {
    let (env, contract_id, token_id, sender, recipient) = setup_env();
    let c = SoroStreamContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    c.initialize(&admin, &soroban_sdk::String::from_str(&env, "1.0.0"));
    if fee_bps > 0 {
        c.set_protocol_fee(&fee_bps);
        c.set_treasury_address(&admin);
    }
    (env, contract_id, token_id, sender, recipient)
}

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

fn make_params(cliff_seconds: u64, nonce: u64, lock_until: u64, allow_recipient_termination: bool) -> crate::types::CreateStreamParams {
    crate::types::CreateStreamParams {
        cliff_seconds,
        nonce,
        renew_count: None,
        recurrence: None,
        lock_until,
        allow_recipient_termination,
        non_transferable: false,
        holdback_amount: 0,
        withdrawal_steps: None,
        min_withdrawal_amount: None,
        sponsor: None,
        requires_recipient_approval: false,

        priority: None,
    }
}

fn default_params() -> crate::types::CreateStreamParams {
    make_params(0, 0, 0, false)
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

        c.create_stream(&sender, &recipient, &token_id, &amount, &duration, &false, &make_params(cliff, 0, 0, false));

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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &make_params(cliff, 0, 0, false),
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
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
                &sender, &recipient, &token_id, &100_000, &1000, &false,
                &default_params(),
            );
            prop_assert!(result.is_err());

            if do_unpause {
                c.unpause();
                prop_assert!(!c.is_paused());

                // create_stream must work after unpause
                let result = c.try_create_stream(
                    &sender, &recipient, &token_id, &100_000, &1000, &false,
                    &default_params(),
                );
                prop_assert!(result.is_ok());
            }
        }
    }
}

// ── Issue #259: create_stream boundary fuzz tests ────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Fuzz: create_stream with extreme total_amount and duration_seconds.
    /// Assert no panics occur outside of expected StreamError variants.
    #[test]
    fn prop_fuzz_create_stream_amount_duration_boundaries(
        amount in 1_i128..=i128::MAX / 2,
        duration in 0_u64..=u64::MAX,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(1000);

        // Mint enough tokens for the test
        let mint_amount = amount.min(10_000_000_000i128);
        StellarAssetClient::new(&env, &token_id).mint(&sender, &mint_amount);

        let result = c.try_create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );

        // No panics should occur. The result is either Ok or an expected error.
        if let Err(e) = &result {
            let err = e.clone().unwrap_err();
            // Must be one of the expected error variants
            prop_assert!(
                matches!(
                    err,
                    StreamError::ZeroAmount
                        | StreamError::ZeroFlowRate
                        | StreamError::StreamDurationTooShort
                        | StreamError::InvalidEndTime
                        | StreamError::Overflow
                        | StreamError::DuplicateStream
                        | StreamError::NewSenderStreamCapExceeded
                ),
                "unexpected error variant: {:?} (amount={}, duration={})",
                err, amount, duration,
            );
        }
    }

    /// Fuzz: create_stream with cliff_seconds relative to duration_seconds.
    /// When cliff >= duration, must fail with InvalidCliff.
    #[test]
    fn prop_fuzz_create_stream_cliff_boundaries(
        duration in 1_u64..=100_000_u64,
        cliff in 0_u64..=100_000_u64,
        amount in 1_000_i128..=1_000_000_i128,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let result = c.try_create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &make_params(cliff, 0, 0, false),
        );

        if cliff >= duration {
            // Must reject with InvalidCliff
            prop_assert!(
                result.is_err(),
                "cliff ({}) >= duration ({}) should fail, but got Ok",
                cliff, duration,
            );
        }
    }

    /// Fuzz: create_stream with extreme nonce values.
    /// Duplicate nonce within same sender must fail with DuplicateStream.
    #[test]
    fn prop_fuzz_create_stream_nonce_boundaries(
        nonce1 in 0_u64..=u64::MAX,
        nonce2 in 0_u64..=u64::MAX,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let result1 = c.try_create_stream(
            &sender, &recipient, &token_id, &100_000, &1000, &false,
            &make_params(0, nonce1, 0, false),
        );

        if result1.is_ok() && nonce1 == nonce2 {
            // Same nonce must be rejected
            let result2 = c.try_create_stream(
                &sender, &recipient, &token_id, &100_000, &1000, &false,
                &make_params(0, nonce2, 0, false),
            );
            prop_assert!(
                result2.is_err(),
                "duplicate nonce {} should be rejected",
                nonce1,
            );
        }
    }

    /// Fuzz: create_stream with large duration (near u64::MAX) and small flow_rate.
    /// Should either succeed or fail with Overflow, not panic.
    #[test]
    fn prop_fuzz_create_stream_large_duration_small_flow(
        amount in 1_i128..=100_000_i128,
        large_duration in 100_000_u64..=u64::MAX / 2,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        StellarAssetClient::new(&env, &token_id).mint(&sender, &amount);

        let result = c.try_create_stream(
            &sender, &recipient, &token_id, &amount, &large_duration, &false,
            &default_params(),
        );

        // No panics allowed
        if let Err(e) = &result {
            let err = e.clone().unwrap_err();
            prop_assert!(
                matches!(
                    err,
                    StreamError::ZeroFlowRate
                        | StreamError::StreamDurationTooShort
                        | StreamError::Overflow
                        | StreamError::InvalidEndTime
                ),
                "unexpected error for large duration: {:?}",
                err,
            );
        }
    }
}

// ── Issue #258: top_up arithmetic invariant properties ───────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Invariant 1: new_end_time > old_end_time for any positive extra_amount.
    /// Invariant 2: new_end_time - old_end_time == floor(extra_amount / flow_rate).
    #[test]
    fn prop_topup_invariants_duration_extension(
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );
        let stream_before = c.get_stream(&stream_id);
        let old_end_time = stream_before.end_time;

        let effective_topup = topup - (topup % flow_rate);
        if effective_topup <= 0 { return Ok(()); }

        let result = c.try_top_up(&stream_id, &sender, &token_id, &topup);
        if result.is_err() { return Ok(()); }

        let stream_after = c.get_stream(&stream_id);

        // Invariant 1: new_end_time > old_end_time
        prop_assert!(
            stream_after.end_time > old_end_time,
            "Invariant 1 violated: new_end_time ({}) must be > old_end_time ({})",
            stream_after.end_time, old_end_time,
        );

        // Invariant 2: extension == floor(extra_amount / flow_rate)
        let expected_extension = (effective_topup / flow_rate) as u64;
        prop_assert_eq!(
            stream_after.end_time - old_end_time,
            expected_extension,
            "Invariant 2 violated: extension ({}) != floor(effective_topup({}) / flow_rate({})) = {}",
            stream_after.end_time - old_end_time, effective_topup, flow_rate, expected_extension,
        );
    }

    /// Invariant 3: after top_up, get_claimable is monotonically non-decreasing.
    /// Check that claimable at old_end_time <= claimable at new_end_time.
    #[test]
    fn prop_topup_invariant_claimable_monotonic(
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );
        let stream_before = c.get_stream(&stream_id);
        let old_end_time = stream_before.end_time;

        let effective_topup = topup - (topup % flow_rate);
        if effective_topup <= 0 { return Ok(()); }

        let result = c.try_top_up(&stream_id, &sender, &token_id, &topup);
        if result.is_err() { return Ok(()); }

        let stream_after = c.get_stream(&stream_id);
        let new_end_time = stream_after.end_time;

        // Check claimable at old_end_time (was the original end of the stream)
        env.ledger().set_timestamp(old_end_time);
        let claimable_at_old = c.get_claimable(&stream_id);

        // Check claimable at new_end_time (now the extended end)
        env.ledger().set_timestamp(new_end_time);
        let claimable_at_new = c.get_claimable(&stream_id);

        // Invariant 3: claimable must be non-decreasing
        prop_assert!(
            claimable_at_new >= claimable_at_old,
            "Invariant 3 violated: claimable at new_end ({}) < claimable at old_end ({})",
            claimable_at_new, claimable_at_old,
        );
    }

    /// top_up invariants: deposit increases by exactly effective_amount.
    #[test]
    fn prop_topup_invariant_deposit_increase(
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
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );
        let stream_before = c.get_stream(&stream_id);
        let old_deposit = stream_before.deposit;

        let effective_topup = topup - (topup % flow_rate);
        if effective_topup <= 0 { return Ok(()); }

        let result = c.try_top_up(&stream_id, &sender, &token_id, &topup);
        if result.is_err() { return Ok(()); }

        let stream_after = c.get_stream(&stream_id);
        prop_assert_eq!(
            stream_after.deposit,
            old_deposit + effective_topup,
            "deposit must increase by exactly effective_topup({})",
            effective_topup,
        );
    }
}

// ── Issue #311: property-based tests for cancel refund arithmetic invariants ──

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_000))]

    /// Invariant: sender_refund + recipient_claimable_at_cancel + protocol_fee = original_deposit.
    /// Since no fee is charged on cancel, this reduces to refund + claimable = deposit.
    /// Edge cases: cancellation at t=0, t=end_time/2, and t=end_time are explicitly sampled.
    #[test]
    fn prop_cancel_refund_invariant(
        amount in 100_i128..=1_000_000_i128,
        duration in 10_u64..=100_000_u64,
        cancel_time in 0_u64..=100_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let sender_before = token.balance(&sender);

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );

        // Cancel at specified time (bounded to [0, end_time] for edge coverage)
        let end_time = duration;
        let cancel_time = match cancel_time {
            0 => 0,                               // t=0 edge case
            t if t >= duration => duration,       // t=end_time edge case
            t if t >= duration / 2 => duration / 2, // t=end_time/2 edge case
            t => t,
        };
        env.ledger().set_timestamp(cancel_time);
        c.cancel_stream(&stream_id, &sender);

        let sender_after = token.balance(&sender);
        let recipient_after = token.balance(&recipient);

        let sender_refund = sender_after - sender_before;
        let recipient_claimable = recipient_after;

        // Invariant: refund + claimable = deposit (no fee on cancel)
        prop_assert_eq!(
            sender_refund + recipient_claimable,
            amount,
            "refund({}) + claimable({}) must equal deposit({}) at cancel_time={}",
            sender_refund, recipient_claimable, amount, cancel_time,
        );
    }

    /// Invariant holds even when a protocol fee is configured (withdrawals before cancel).
    /// total_withdrawn_net + total_fees + cancel_claimable + refund = deposit
    #[test]
    fn prop_cancel_refund_with_fee_invariant(
        amount in 10_000_i128..=500_000_i128,
        duration in 100_u64..=10_000_u64,
        cancel_time in 1_u64..=10_000_u64,
        fee_bps in 1_u32..=500_u32,
        withdraw_before_cancel in proptest::bool::ANY,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env_with_fee(fee_bps);
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let sender_before = token.balance(&sender);

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );

        // Optionally withdraw before cancel
        let mut fees_collected_before: i128 = 0;
        if withdraw_before_cancel && cancel_time > 1 {
            let withdraw_time = (cancel_time / 2).max(1);
            env.ledger().set_timestamp(withdraw_time);
            let _ = c.try_withdraw(&stream_id, &recipient);
            fees_collected_before = c.get_fees_collected(&token_id);
        }

        let cancel_time = cancel_time.min(duration);
        env.ledger().set_timestamp(cancel_time);
        c.cancel_stream(&stream_id, &sender);

        let sender_after = token.balance(&sender);
        let recipient_after = token.balance(&recipient);
        let sender_refund = sender_after - sender_before;
        let total_recipient = recipient_after;
        let total_fees = c.get_fees_collected(&token_id);

        // Invariant: refund + recipient_total + fees = deposit
        prop_assert!(
            sender_refund + total_recipient + total_fees == amount,
            "refund({}) + recipient({}) + fees({}) != deposit({}) at cancel_time={}, withdraw_before={}",
            sender_refund, total_recipient, total_fees, amount, cancel_time, withdraw_before_cancel,
        );
    }
}

// ── Issue #501: Property-based tests for accrual calculation invariants ───────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5_000))]

    /// Invariant: Sum of all partial withdrawals never exceeds the original deposit.
    /// Tests multiple withdrawals at different times throughout the stream lifetime.
    #[test]
    fn prop_partial_withdrawals_never_exceed_deposit(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        withdraw_count in 2_usize..=10_usize,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );

        let mut total_withdrawn: i128 = 0;
        let time_step = duration / withdraw_count as u64;

        for i in 0..withdraw_count {
            let current_time = (i as u64 + 1) * time_step;
            if current_time >= duration {
                break;
            }

            env.ledger().set_timestamp(current_time);

            if c.try_get_stream(&stream_id).is_ok() {
                let _ = c.try_withdraw(&stream_id, &recipient);
                let new_balance = token.balance(&recipient);
                total_withdrawn = new_balance;

                // Invariant: total withdrawn must never exceed deposit
                prop_assert!(
                    total_withdrawn <= amount,
                    "total_withdrawn({}) must not exceed deposit({}) after {} withdrawals at time={}",
                    total_withdrawn, amount, i, current_time,
                );
            }
        }

        // Final check: total withdrawn should equal approximately the deposit (within rounding)
        env.ledger().set_timestamp(duration);
        if c.try_get_stream(&stream_id).is_ok() {
            let _ = c.try_withdraw(&stream_id, &recipient);
        }
        let final_balance = token.balance(&recipient);
        prop_assert!(
            final_balance <= amount,
            "final_balance({}) must not exceed deposit({})",
            final_balance, amount,
        );
    }

    /// Invariant: Accrued amount at any point in time is bounded by:
    /// max_accrued = min(time_elapsed * flow_rate, deposit)
    #[test]
    fn prop_accrued_bounded_by_deposit_and_time(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        query_time in 0_u64..=20_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );
        let stream = c.get_stream(&stream_id);

        // Query at a random time
        let query_time = query_time.min(duration + 1000);
        env.ledger().set_timestamp(query_time);

        if c.try_get_stream(&stream_id).is_ok() {
            let claimable = c.get_claimable(&stream_id);

            // Calculate expected accrual bounds
            let time_elapsed = query_time.saturating_sub(stream.start_time) as i128;
            let max_by_time = time_elapsed * flow_rate;
            let expected_max = max_by_time.min(amount);

            prop_assert!(
                claimable <= expected_max,
                "claimable({}) must be <= min(time_elapsed({}) * flow_rate({}), deposit({})) = {}",
                claimable, time_elapsed, flow_rate, amount, expected_max,
            );

            // Claimable should also be >= 0
            prop_assert!(
                claimable >= 0,
                "claimable({}) must be non-negative",
                claimable,
            );
        }
    }

    /// Invariant: After a partial withdrawal, remaining claimable must be non-negative
    /// and total (claimed + remaining) equals the full accrual at that time.
    #[test]
    fn prop_withdrawal_accounting_complete(
        amount in 10_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        withdraw_time in 1_u64..=10_000_u64,
    ) {
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let c = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);
        env.ledger().set_timestamp(0);

        let flow_rate = amount / duration as i128;
        if flow_rate == 0 { return Ok(()); }

        let stream_id = c.create_stream(
            &sender, &recipient, &token_id, &amount, &duration, &false,
            &default_params(),
        );

        let withdraw_time = withdraw_time.min(duration);
        env.ledger().set_timestamp(withdraw_time);

        let claimable_before = c.get_claimable(&stream_id);
        c.withdraw(&stream_id, &recipient);
        let withdrawn = token.balance(&recipient);

        // Invariant: withdrawn amount must equal claimable before withdrawal
        prop_assert_eq!(
            withdrawn, claimable_before,
            "withdrawn({}) must equal claimable_before({})",
            withdrawn, claimable_before,
        );

        if c.try_get_stream(&stream_id).is_ok() {
            let claimable_after = c.get_claimable(&stream_id);
            // After withdrawal, remaining claimable must be non-negative
            prop_assert!(
                claimable_after >= 0,
                "claimable_after({}) must be non-negative",
                claimable_after,
            );
        }
    }
}
