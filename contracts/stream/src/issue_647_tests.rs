#![cfg(test)]

use super::*;
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String,
};

const FEE_TIMELOCK_SECONDS: u64 = 48 * 60 * 60;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_fee_withdrawal_and_refund_conserve_balance(
        fee_bps in 0_u32..=10_000_u32,
        amount in 20_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        elapsed in 1_u64..=10_000_u64,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(SoroStreamContract, ());
        let token_admin = Address::generate(&env);
        let token_id = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        let sender = Address::generate(&env);
        let recipient = Address::generate(&env);
        let treasury = Address::generate(&env);
        let client = SoroStreamContractClient::new(&env, &contract_id);
        let token = TokenClient::new(&env, &token_id);

        StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);
        client.initialize(&sender, &String::from_str(&env, "1.0.0"));
        client.set_min_duration(&sender, &0_u64);
        client.set_treasury_address(&treasury);

        env.ledger().set_timestamp(0);
        client.set_protocol_fee(&fee_bps);
        env.ledger().set_timestamp(FEE_TIMELOCK_SECONDS);
        client.execute_fee_change();

        let stream_start = env.ledger().timestamp();
        let params = CreateStreamParams {
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
        };
        let sender_before = token.balance(&sender);
        let stream_id = client.create_stream(
            &sender,
            &recipient,
            &token_id,
            &amount,
            &duration,
            &false,
            &params,
        );

        let withdrawal_time = elapsed.min(duration - 1);
        env.ledger().set_timestamp(stream_start + withdrawal_time);
        client.withdraw(&stream_id, &recipient);
        client.cancel_stream(&stream_id, &sender);

        let sender_after = token.balance(&sender);
        let recipient_after = token.balance(&recipient);
        let collected_fee = client.get_fees_collected(&token_id);

        prop_assert_eq!(
            sender_before,
            sender_after + recipient_after + collected_fee,
            "sender refund, recipient payout, and protocol fee must conserve the initial balance"
        );
        prop_assert_eq!(
            token.balance(&contract_id),
            collected_fee,
            "the contract must retain exactly the accounted fee after cancellation"
        );
    }
}
