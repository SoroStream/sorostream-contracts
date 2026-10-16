//! Tests for governance `create_proposal` threshold boundary (issue #773).
//!
//! Pins down the exact boundary: a proposer whose balance is exactly equal to
//! `proposal_threshold` must be accepted, while one unit below must be rejected
//! with `InsufficientTokens`.

extern crate std;

use crate::{GovernanceContract, GovernanceContractClient, GovernanceError};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, Env, Symbol,
};

const THRESHOLD: u64 = 1_000_000;

struct TestEnv {
    env: Env,
    client: GovernanceContractClient<'static>,
    token: Address,
    proposer: Address,
    target: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovernanceContract, ());
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let proposer = Address::generate(&env);
    let target = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let client = GovernanceContractClient::new(&env, &contract_id);
    client.initialize(
        &admin,
        &guardian,
        &token,
        &THRESHOLD,
        &1u64, // quorum
        &0u64, // voting_period -> default
        &0u64, // timelock_period -> default
    );

    TestEnv {
        env,
        client,
        token,
        proposer,
        target,
    }
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn try_create(t: &TestEnv) -> Result<Result<u64, soroban_sdk::Error>, Result<GovernanceError, soroban_sdk::InvokeError>> {
    t.client.try_create_proposal(
        &t.proposer,
        &t.target,
        &Symbol::new(&t.env, "do_thing"),
        &Bytes::new(&t.env),
    )
}

/// Balance exactly equal to threshold must be accepted.
#[test]
fn test_create_proposal_balance_exactly_at_threshold_succeeds() {
    let t = setup();
    mint(&t.env, &t.token, &t.proposer, THRESHOLD as i128);

    let balance = TokenClient::new(&t.env, &t.token).balance(&t.proposer);
    assert_eq!(balance, THRESHOLD as i128);

    let res = try_create(&t);
    match res {
        Ok(Ok(id)) => assert_eq!(id, 0),
        other => panic!("expected Ok(Ok(0)), got {:?}", other),
    }
}

/// Balance one unit below threshold must be rejected with InsufficientTokens.
#[test]
fn test_create_proposal_balance_one_below_threshold_rejected() {
    let t = setup();
    mint(&t.env, &t.token, &t.proposer, (THRESHOLD - 1) as i128);

    let balance = TokenClient::new(&t.env, &t.token).balance(&t.proposer);
    assert_eq!(balance, (THRESHOLD - 1) as i128);

    let res = try_create(&t);
    match res {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::InsufficientTokens),
        other => panic!("expected InsufficientTokens, got {:?}", other),
    }
}