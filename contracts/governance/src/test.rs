#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn default_token(env: &Env) -> Address {
    Address::generate(env)
}

#[test]
fn test_set_guardian_single_step_still_works() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let new_guardian = Address::generate(&env);
    c.set_guardian(&new_guardian);

    let stored: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY))
    });
    assert_eq!(stored, Some(new_guardian));
}

#[test]
fn test_guardian_two_step_transfer() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let new_guardian = Address::generate(&env);

    // No pending guardian before proposal.
    let pending: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY))
    });
    assert!(pending.is_none());

    // Admin proposes the new guardian.
    c.propose_guardian(&new_guardian);

    let pending: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY))
    });
    assert_eq!(pending, Some(new_guardian.clone()));

    // Guardian is not yet switched.
    let stored: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY))
    });
    assert_eq!(stored, Some(guardian.clone()));

    // The pending guardian accepts, completing the two-step transfer.
    c.accept_guardian(&new_guardian);

    let stored: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY))
    });
    assert_eq!(stored, Some(new_guardian));

    // Pending is cleared after acceptance.
    let pending: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY))
    });
    assert!(pending.is_none());
}

#[test]
fn test_accept_guardian_without_proposal_fails() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let new_guardian = Address::generate(&env);
    let result = c.try_accept_guardian(&new_guardian);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::NotInitialized),
        _ => panic!("expected NotInitialized"),
    }
}

#[test]
fn test_accept_guardian_wrong_caller_fails() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let new_guardian = Address::generate(&env);
    let impostor = Address::generate(&env);

    c.propose_guardian(&new_guardian);

    let result = c.try_accept_guardian(&impostor);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::NotAdmin),
        _ => panic!("expected NotAdmin"),
    }
}

#[test]
fn test_propose_guardian_twice_overwrites_pending() {
    // Issue #895: propose_guardian() unconditionally overwrites
    // PENDING_GUARDIAN_KEY, so a second proposal silently replaces the first
    // before either is accepted.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token = default_token(&env);

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let first = Address::generate(&env);
    let second = Address::generate(&env);

    // Propose `first`, then propose `second` before `first` accepts.
    c.propose_guardian(&first);
    c.propose_guardian(&second);

    // The second proposal overwrote the first as the only pending guardian.
    let pending: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY))
    });
    assert_eq!(pending, Some(second.clone()));

    // `first` is no longer pending, so accepting as `first` fails.
    let result = c.try_accept_guardian(&first);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, GovernanceError::NotAdmin),
        _ => panic!("expected NotAdmin"),
    }

    // `second` is the only address accept_guardian will honor.
    c.accept_guardian(&second);

    let stored: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::GUARDIAN_KEY))
    });
    assert_eq!(stored, Some(second));

    // Pending is cleared after acceptance.
    let pending: Option<Address> = env.as_contract(&id, || {
        env.storage()
            .instance()
            .get(&soroban_sdk::Symbol::new(&env, super::PENDING_GUARDIAN_KEY))
    });
    assert!(pending.is_none());
}

#[test]
fn test_queue_quorum_boundary_evenly_split() {
    // Issue #824: Test boundary where total_votes exactly equals quorum
    // but votes_for equals votes_against (tie at exactly quorum threshold).
    // The queue() function requires: total_votes >= quorum AND votes_for > votes_against
    // This test ensures the proposal is Defeated, not accidentally passed.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let quorum = 4u64; // Set quorum to 4 votes

    c.initialize(&admin, &guardian, &token, &0u64, &quorum, &0u64, &0u64);
    env.ledger().set_timestamp(1000);

    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "dummy");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);

    // Create a proposal: total_votes = 4 (exactly at quorum), split 2 for / 2 against
    let prop_id = c.create_proposal(&admin, &target, &function, &calldata);

    // Vote 2 for, 2 against (exactly equal, exactly at quorum). Each voter's
    // weight equals their token balance, so mint 1 token (1 unit of weight)
    // to each before voting.
    let voter1 = Address::generate(&env);
    let voter2 = Address::generate(&env);
    let voter3 = Address::generate(&env);
    let voter4 = Address::generate(&env);
    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    for voter in [&voter1, &voter2, &voter3, &voter4] {
        stellar_asset.mint(voter, &1);
    }

    c.vote(&voter1, &prop_id, &true); // vote_for
    c.vote(&voter2, &prop_id, &true); // vote_for
    c.vote(&voter3, &prop_id, &false); // vote_against
    c.vote(&voter4, &prop_id, &false); // vote_against

    // Advance past the voting period, then try to queue: the call succeeds
    // (Ok), but the proposal resolves to Defeated, not Queued, because
    // votes_for (2) == votes_against (2), tie not strictly >.
    env.ledger().set_timestamp(env.ledger().timestamp() + 1_000_000);
    let result = c.try_queue(&prop_id);
    assert_eq!(result, Ok(Ok(())));
    let proposal = c.get_proposal(&prop_id);
    assert_eq!(
        proposal.status,
        ProposalStatus::Defeated,
        "Tied proposal at quorum boundary should be Defeated, not Queued"
    );
}

#[test]
fn test_withdraw_vote_tokens_twice_fails_with_no_vote_to_withdraw() {
    // Issue #983: withdraw_vote_tokens removes the locked_balance_key entry
    // immediately after reading it, so a second withdrawal for the same
    // voter/proposal pair must fail with NoVoteToWithdraw.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);
    env.ledger().set_timestamp(1000);

    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "dummy");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);
    let prop_id = c.create_proposal(&admin, &target, &function, &calldata);

    let voter = Address::generate(&env);
    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    let token_client = soroban_sdk::token::Client::new(&env, &token);
    stellar_asset.mint(&voter, &100);

    c.vote(&voter, &prop_id, &true);
    assert_eq!(token_client.balance(&voter), 0);
    assert_eq!(token_client.balance(&id), 100);

    // Move past vote_end so withdrawal is permitted.
    let proposal = c.get_proposal(&prop_id);
    env.ledger().set_timestamp(proposal.vote_end + 1);

    // First withdrawal succeeds and returns the escrowed tokens.
    c.withdraw_vote_tokens(&voter, &prop_id);
    assert_eq!(token_client.balance(&voter), 100);
    assert_eq!(token_client.balance(&id), 0);

    // The locked balance entry has been removed from persistent storage.
    let locked: Option<u64> = env.as_contract(&id, || {
        env.storage()
            .persistent()
            .get(&super::locked_balance_key(&env, prop_id, &voter))
    });
    assert!(locked.is_none());

    // Second withdrawal for the same voter/proposal fails.
    let result = c.try_withdraw_vote_tokens(&voter, &prop_id);
    assert_eq!(result, Err(Ok(GovernanceError::NoVoteToWithdraw)));

    // Balances are unchanged by the failed second call.
    assert_eq!(token_client.balance(&voter), 100);
    assert_eq!(token_client.balance(&id), 0);
}

#[test]
fn test_create_proposal_proposer_balance_exceeds_u64_max() {
    // Issue #893: create_proposal converts proposer balance via
    // u64::try_from(balance).map_err(|_| GovernanceError::InvalidParam)?
    // Confirm create_proposal is rejected as InvalidParam when proposer balance > u64::MAX
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    c.initialize(&admin, &guardian, &token, &100u64, &10u64, &0u64, &0u64);

    let proposer = Address::generate(&env);
    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    let excess_balance: i128 = (u64::MAX as i128) + 1;
    stellar_asset.mint(&proposer, &excess_balance);

    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "dummy");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);

    let result = c.try_create_proposal(&proposer, &target, &function, &calldata);
    assert_eq!(result, Err(Ok(GovernanceError::InvalidParam)));
}

