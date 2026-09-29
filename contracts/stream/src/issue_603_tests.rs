//! Tests for issue #603: `withdraw` must reject when the claimable amount
//! would overflow the recipient's token balance.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

/// When the recipient's balance is already near `i128::MAX`, a withdrawal that
/// would push it over must be rejected with `RecipientBalanceOverflow`.
#[test]
fn test_withdraw_recipient_balance_overflow_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    let asset_client = StellarAssetClient::new(&env, &token_id);

    // Mint sender enough to create the stream.
    asset_client.mint(&sender, &100_000i128);

    // Fill recipient to near i128::MAX so any withdrawal overflows.
    // i128::MAX = 170_141_183_460_469_231_731_687_303_715_884_105_727
    // We set recipient balance to i128::MAX - 1 so adding even 1 stroop overflows.
    asset_client.mint(&recipient, &(i128::MAX - 1));

    let c = SoroStreamContractClient::new(&env, &contract_id);
    c.set_min_duration(&sender, &0u64);

    let stream_id = c.create_stream(
        &sender,
        &recipient,
        &token_id,
        &100_000,
        &1000,
        &0,
        &0u64,
        &false,
        &0u64,
        &false,
        &0i128,
        &None::<u32>,
        &None::<i128>,
        &None::<u32>,
    );

    // Advance time so there is something claimable.
    env.ledger().set_timestamp(100);

    // Withdrawal should be rejected with RecipientBalanceOverflow.
    let result = c.try_withdraw(&stream_id, &recipient);
    assert_eq!(
        result,
        Err(Ok(StreamError::RecipientBalanceOverflow)),
        "withdraw must fail with RecipientBalanceOverflow when recipient balance would overflow"
    );
}
