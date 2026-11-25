#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, IntoVal, TryFromVal,
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
fn test_accept_guardian_replay_after_success_fails() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    let new_guardian = Address::generate(&env);
    c.propose_guardian(&new_guardian);
    c.accept_guardian(&new_guardian);

    let replay = c.try_accept_guardian(&new_guardian);
    assert_eq!(replay, Err(Ok(GovernanceError::NotInitialized)));
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
fn test_cancel_after_voting_period_is_rejected() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);

    env.ledger().set_timestamp(1_000);
    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "noop");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);
    let proposal_id = c.create_proposal(&admin, &target, &function, &calldata);
    let proposal = c.get_proposal(&proposal_id);

    env.ledger().set_timestamp(proposal.vote_end + 1);
    let result = c.try_cancel_proposal(&admin, &proposal_id);
    assert_eq!(result, Err(Ok(GovernanceError::VotingNotActive)));
    assert_eq!(c.get_proposal(&proposal_id).status, ProposalStatus::Active);
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
fn test_execute_failed_proposal_can_be_retried_successfully() {
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &1u64, &1u64);
    env.ledger().set_timestamp(1_000);

    // The first execution targets accept_guardian before any pending transfer
    // exists, so it fails. The admin then creates the pending transfer and the
    // same proposal succeeds on its second execution.
    let new_guardian = Address::generate(&env);
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::from_array(
        &env,
        [new_guardian.clone().into_val(&env)],
    );
    let prop_id = c.create_proposal(
        &admin,
        &id,
        &soroban_sdk::Symbol::new(&env, "accept_guardian"),
        &calldata,
    );

    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    stellar_asset.mint(&admin, &1);
    c.vote(&admin, &prop_id, &true);
    let proposal = c.get_proposal(&prop_id);
    env.ledger().set_timestamp(proposal.vote_end + 1);
    c.queue(&prop_id);
    let queued = c.get_proposal(&prop_id);
    env.ledger().set_timestamp(queued.eta);

    let first = c.try_execute(&prop_id);
    assert_eq!(first, Err(Ok(GovernanceError::InvokeFailed)));
    assert_eq!(c.get_proposal(&prop_id).status, ProposalStatus::Failed);

    c.propose_guardian(&new_guardian);
    c.execute(&prop_id);
    assert_eq!(c.get_proposal(&prop_id).status, ProposalStatus::Executed);

    let executed_event = env.events().all().iter().any(|(address, topics, _)| {
        if address != id {
            return false;
        }
        topics
            .get(0)
            .and_then(|topic| soroban_sdk::Symbol::try_from_val(&env, &topic).ok())
            == Some(soroban_sdk::Symbol::new(&env, "ProposalExecuted"))
    });
    assert!(executed_event);
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
fn test_veto_by_proposer_who_is_not_guardian_is_rejected(
) {
    // Issue #919: `veto()` rejects any caller who is not the guardian with
    // `GovernanceError::NotGuardian`. Being the proposer grants no special
    // veto rights — this test pins that invariant by having the proposer
    // (a distinct address from the guardian) attempt to veto their own
    // queued proposal.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let proposer = Address::generate(&env);
    assert_ne!(
        proposer, guardian,
        "test setup error: proposer and guardian must be distinct addresses"
    );
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);
    env.ledger().set_timestamp(1000);

    // Fund the proposer so `create_proposal` clears the threshold check
    // (threshold is 0 here, but minting makes the intent explicit).
    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    stellar_asset.mint(&proposer, &1);

    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "dummy");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);
    let prop_id = c.create_proposal(&proposer, &target, &function, &calldata);

    // Drive the proposal through voting into Queued so veto has a
    // non-terminal proposal to act on. Mint to the proposer again (balance
    // was escrowed during their own vote, if any) and cast one vote_for.
    c.vote(&proposer, &prop_id, &true);
    env.ledger().set_timestamp(env.ledger().timestamp() + 1_000_000);
    c.queue(&prop_id);
    let queued = c.get_proposal(&prop_id);
    assert_eq!(queued.status, ProposalStatus::Queued);

    // The proposer attempts to veto their own proposal — must fail with
    // NotGuardian. Any other outcome means `veto()` is granting veto
    // authority to the proposer, which the guardian-only check forbids.
    let result = c.try_veto(&proposer, &prop_id);
    assert_eq!(result, Err(Ok(GovernanceError::NotGuardian)));

    // Status unchanged; the failed veto did not advance the proposal state.
    let unchanged = c.get_proposal(&prop_id);
    assert_eq!(unchanged.status, ProposalStatus::Queued);

    // Control check: the guardian CAN veto the same proposal, proving the
    // rejection above was specifically about the caller's identity and not
    // about the proposal being in an unvetoable state.
    c.veto(&guardian, &prop_id);
    let vetoed = c.get_proposal(&prop_id);
    assert_eq!(vetoed.status, ProposalStatus::Vetoed);
}

#[test]
fn test_queue_with_zero_quorum_snapshot_passes_on_any_positive_margin() {
    // Issue #921: `create_proposal` snapshots the governance-wide `quorum`
    // onto the proposal at creation time. If `quorum == 0` at creation,
    // `queue()`'s `total_votes < proposal.quorum` check is always false,
    // so the proposal can queue with any positive margin — including a
    // proposal where only one voter voted_for.
    //
    // Pins that:
    //   - the quorum check is NOT re-read at queue() time (raising the
    //     governance-wide quorum after creation does not retroactively
    //     block queuing);
    //   - the 0-vs-0 case still Defeats, because `votes_for <= votes_against`
    //     catches the tie.
    let env = env_with_auth();
    let id = env.register(GovernanceContract, ());
    let c = GovernanceContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    // Initialize with quorum = 0 so proposals created now snapshot a
    // zero quorum requirement.
    c.initialize(&admin, &guardian, &token, &0u64, &0u64, &0u64, &0u64);
    env.ledger().set_timestamp(1000);

    let target = Address::generate(&env);
    let function = soroban_sdk::Symbol::new(&env, "dummy");
    let calldata: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);
    let prop_id = c.create_proposal(&admin, &target, &function, &calldata);

    // Confirm the snapshot really did capture quorum = 0.
    let created = c.get_proposal(&prop_id);
    assert_eq!(created.quorum, 0);

    // One voter votes_for with balance 1; total_votes = 1, > 0 = quorum.
    let voter = Address::generate(&env);
    let stellar_asset = soroban_sdk::token::StellarAssetClient::new(&env, &token);
    stellar_asset.mint(&voter, &1);
    c.vote(&voter, &prop_id, &true);

    env.ledger().set_timestamp(env.ledger().timestamp() + 1_000_000);
    let result = c.try_queue(&prop_id);
    assert_eq!(result, Ok(Ok(())));
    let queued = c.get_proposal(&prop_id);
    assert_eq!(
        queued.status,
        ProposalStatus::Queued,
        "Proposal with zero-quorum snapshot and positive margin must queue"
    );

    // Second proposal: still zero-quorum, but nobody votes. `votes_for (0)
    // <= votes_against (0)` catches the tie and resolves to Defeated.
    env.ledger().set_timestamp(2000);
    let zero_votes_id = c.create_proposal(&admin, &target, &function, &calldata);
    let zero_created = c.get_proposal(&zero_votes_id);
    assert_eq!(zero_created.quorum, 0);

    env.ledger().set_timestamp(env.ledger().timestamp() + 1_000_000);
    let result = c.try_queue(&zero_votes_id);
    assert_eq!(result, Ok(Ok(())));
    let zero_queued = c.get_proposal(&zero_votes_id);
    assert_eq!(
        zero_queued.status,
        ProposalStatus::Defeated,
        "Zero-quorum proposal with zero votes is a 0-vs-0 tie and must Defeat"
    );
}
