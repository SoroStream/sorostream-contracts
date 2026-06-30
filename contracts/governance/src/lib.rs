#![no_std]
//! # SoroStream Governance Contract
//!
//! On-chain governance for protocol parameter changes.
//!
//! ## Flow
//! 1. Any address holding >= `proposal_threshold` tokens calls `create_proposal`.
//! 2. Token holders call `vote` during the voting period.
//! 3. After the voting period ends, if quorum is met, anyone calls `queue` to start the timelock.
//! 4. After the 48-hour timelock the proposal is executable via `execute`.
//! 5. The guardian may `veto` a queued proposal before it executes.

use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Bytes, Env, Symbol, Vec,
};

// ── Storage keys ─────────────────────────────────────────────────────────────

const ADMIN_KEY: &str = "admin";
const GUARDIAN_KEY: &str = "guardian";
const TOKEN_KEY: &str = "token";
const PROPOSAL_THRESHOLD_KEY: &str = "p_thresh";
const QUORUM_KEY: &str = "quorum";
const VOTING_PERIOD_KEY: &str = "vp";
const TIMELOCK_PERIOD_KEY: &str = "tlp";
const PROPOSAL_COUNT_KEY: &str = "p_cnt";

const DEFAULT_VOTING_PERIOD: u64 = 7 * 24 * 60 * 60; // 7 days
const DEFAULT_TIMELOCK_PERIOD: u64 = 48 * 60 * 60;   // 48 hours

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    /// Voting is open.
    Active,
    /// Voting ended but quorum was not reached.
    Defeated,
    /// Quorum reached; waiting out the timelock.
    Queued,
    /// Timelock elapsed; proposal executed.
    Executed,
    /// Vetoed by the guardian during the timelock.
    Vetoed,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    /// Target contract to invoke on execution.
    pub target: Address,
    /// Function name to call on the target.
    pub function: Symbol,
    /// ABI-encoded arguments passed verbatim to the target.
    pub calldata: Bytes,
    pub status: ProposalStatus,
    /// Ledger timestamp when voting opened.
    pub vote_start: u64,
    /// Ledger timestamp when voting closes.
    pub vote_end: u64,
    /// Ledger timestamp after which execution is allowed (0 until queued).
    pub eta: u64,
    /// Total yes-vote weight.
    pub votes_for: u64,
    /// Total no-vote weight.
    pub votes_against: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GovernanceError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotGuardian = 4,
    ProposalNotFound = 5,
    VotingNotActive = 6,
    AlreadyVoted = 7,
    QuorumNotReached = 8,
    TimelockNotExpired = 9,
    ProposalNotQueued = 10,
    InsufficientTokens = 11,
    InvalidParam = 12,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

fn check_admin(env: &Env) {
    read_admin(env)
        .expect("governance not initialized")
        .require_auth();
}

fn proposal_key(id: u64) -> u64 {
    id
}

fn save_proposal(env: &Env, proposal: &Proposal) {
    env.storage().persistent().set(&proposal_key(proposal.id), proposal);
}

fn load_proposal(env: &Env, id: u64) -> Option<Proposal> {
    env.storage().persistent().get(&proposal_key(id))
}

fn next_proposal_id(env: &Env) -> u64 {
    let key = Symbol::new(env, PROPOSAL_COUNT_KEY);
    let id: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    env.storage().instance().set(&key, &(id + 1));
    id
}

fn voted_key(env: &Env, proposal_id: u64, voter: &Address) -> (Symbol, u64, Address) {
    (Symbol::new(env, "voted"), proposal_id, voter.clone())
}

fn has_voted(env: &Env, proposal_id: u64, voter: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&voted_key(env, proposal_id, voter))
        .unwrap_or(false)
}

fn mark_voted(env: &Env, proposal_id: u64, voter: &Address) {
    env.storage()
        .persistent()
        .set(&voted_key(env, proposal_id, voter), &true);
}

fn get_u64(env: &Env, key: &str, default: u64) -> u64 {
    env.storage().instance().get(&Symbol::new(env, key)).unwrap_or(default)
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialises the governance contract.
    ///
    /// * `token` — governance token used to check balances and vote weight.
    /// * `proposal_threshold` — minimum token balance required to create a proposal.
    /// * `quorum` — minimum total votes (for + against) required for a proposal to pass.
    /// * `voting_period` — seconds the voting window stays open (0 = default 7 days).
    /// * `timelock_period` — seconds between queue and execution (0 = default 48 h).
    pub fn initialize(
        env: Env,
        admin: Address,
        guardian: Address,
        token: Address,
        proposal_threshold: u64,
        quorum: u64,
        voting_period: u64,
        timelock_period: u64,
    ) -> Result<(), GovernanceError> {
        if read_admin(&env).is_some() {
            return Err(GovernanceError::AlreadyInitialized);
        }
        env.storage().instance().set(&Symbol::new(&env, ADMIN_KEY), &admin);
        env.storage().instance().set(&Symbol::new(&env, GUARDIAN_KEY), &guardian);
        env.storage().instance().set(&Symbol::new(&env, TOKEN_KEY), &token);
        env.storage().instance().set(&Symbol::new(&env, PROPOSAL_THRESHOLD_KEY), &proposal_threshold);
        env.storage().instance().set(&Symbol::new(&env, QUORUM_KEY), &quorum);
        let vp = if voting_period == 0 { DEFAULT_VOTING_PERIOD } else { voting_period };
        let tlp = if timelock_period == 0 { DEFAULT_TIMELOCK_PERIOD } else { timelock_period };
        env.storage().instance().set(&Symbol::new(&env, VOTING_PERIOD_KEY), &vp);
        env.storage().instance().set(&Symbol::new(&env, TIMELOCK_PERIOD_KEY), &tlp);
        env.events().publish(
            (Symbol::new(&env, "GovernanceInitialized"),),
            (admin, guardian, token, proposal_threshold, quorum, vp, tlp),
        );
        Ok(())
    }

    /// Creates a new governance proposal.
    ///
    /// The proposer must hold at least `proposal_threshold` tokens.
    pub fn create_proposal(
        env: Env,
        proposer: Address,
        target: Address,
        function: Symbol,
        calldata: Bytes,
    ) -> Result<u64, GovernanceError> {
        proposer.require_auth();

        let token: Address = env.storage().instance()
            .get(&Symbol::new(&env, TOKEN_KEY))
            .ok_or(GovernanceError::NotInitialized)?;
        let threshold = get_u64(&env, PROPOSAL_THRESHOLD_KEY, 0);

        let balance = token::Client::new(&env, &token).balance(&proposer);
        if (balance as u64) < threshold {
            return Err(GovernanceError::InsufficientTokens);
        }

        let now = env.ledger().timestamp();
        let voting_period = get_u64(&env, VOTING_PERIOD_KEY, DEFAULT_VOTING_PERIOD);

        let id = next_proposal_id(&env);
        let proposal = Proposal {
            id,
            proposer: proposer.clone(),
            target: target.clone(),
            function: function.clone(),
            calldata: calldata.clone(),
            status: ProposalStatus::Active,
            vote_start: now,
            vote_end: now.saturating_add(voting_period),
            eta: 0,
            votes_for: 0,
            votes_against: 0,
        };
        save_proposal(&env, &proposal);

        env.events().publish(
            (Symbol::new(&env, "ProposalCreated"), id),
            (proposer, target, function, now, proposal.vote_end),
        );

        Ok(id)
    }

    /// Casts a vote on an active proposal.
    ///
    /// Vote weight equals the voter's current token balance (1 token = 1 vote).
    /// Each address may vote only once per proposal.
    pub fn vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        support: bool,
    ) -> Result<(), GovernanceError> {
        voter.require_auth();

        let mut proposal = load_proposal(&env, proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        let now = env.ledger().timestamp();
        if proposal.status != ProposalStatus::Active
            || now < proposal.vote_start
            || now > proposal.vote_end
        {
            return Err(GovernanceError::VotingNotActive);
        }

        if has_voted(&env, proposal_id, &voter) {
            return Err(GovernanceError::AlreadyVoted);
        }

        let token: Address = env.storage().instance()
            .get(&Symbol::new(&env, TOKEN_KEY))
            .ok_or(GovernanceError::NotInitialized)?;
        let balance = token::Client::new(&env, &token).balance(&voter) as u64;

        mark_voted(&env, proposal_id, &voter);

        if support {
            proposal.votes_for = proposal.votes_for.saturating_add(balance);
        } else {
            proposal.votes_against = proposal.votes_against.saturating_add(balance);
        }
        save_proposal(&env, &proposal);

        env.events().publish(
            (Symbol::new(&env, "VoteCast"), proposal_id),
            (voter, support, balance),
        );

        Ok(())
    }

    /// Queues a passed proposal into the timelock.
    ///
    /// Callable by anyone after voting ends. Fails if quorum was not reached or the
    /// proposal did not receive more yes votes than no votes.
    pub fn queue(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        let mut proposal = load_proposal(&env, proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        let now = env.ledger().timestamp();
        if proposal.status != ProposalStatus::Active || now <= proposal.vote_end {
            return Err(GovernanceError::VotingNotActive);
        }

        let quorum = get_u64(&env, QUORUM_KEY, 0);
        let total_votes = proposal.votes_for.saturating_add(proposal.votes_against);
        if total_votes < quorum || proposal.votes_for <= proposal.votes_against {
            proposal.status = ProposalStatus::Defeated;
            save_proposal(&env, &proposal);
            env.events().publish(
                (Symbol::new(&env, "ProposalDefeated"), proposal_id),
                (proposal.votes_for, proposal.votes_against, total_votes),
            );
            return Err(GovernanceError::QuorumNotReached);
        }

        let timelock = get_u64(&env, TIMELOCK_PERIOD_KEY, DEFAULT_TIMELOCK_PERIOD);
        proposal.eta = now.saturating_add(timelock);
        proposal.status = ProposalStatus::Queued;
        save_proposal(&env, &proposal);

        env.events().publish(
            (Symbol::new(&env, "ProposalQueued"), proposal_id),
            (proposal.eta,),
        );

        Ok(())
    }

    /// Executes a queued proposal after the timelock has elapsed.
    ///
    /// Callable by anyone. Invokes `proposal.function` on `proposal.target` with
    /// `proposal.calldata` as arguments.
    pub fn execute(env: Env, proposal_id: u64) -> Result<(), GovernanceError> {
        let mut proposal = load_proposal(&env, proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Queued {
            return Err(GovernanceError::ProposalNotQueued);
        }

        let now = env.ledger().timestamp();
        if now < proposal.eta {
            return Err(GovernanceError::TimelockNotExpired);
        }

        proposal.status = ProposalStatus::Executed;
        save_proposal(&env, &proposal);

        env.invoke_contract::<()>(
            &proposal.target,
            &proposal.function,
            Vec::from_array(&env, [soroban_sdk::IntoVal::into_val(&proposal.calldata, &env)]),
        );

        env.events().publish(
            (Symbol::new(&env, "ProposalExecuted"), proposal_id),
            (),
        );

        Ok(())
    }

    /// Vetoes a queued proposal. Only the guardian may call this.
    pub fn veto(env: Env, guardian: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        guardian.require_auth();

        let stored: Address = env.storage().instance()
            .get(&Symbol::new(&env, GUARDIAN_KEY))
            .ok_or(GovernanceError::NotInitialized)?;
        if guardian != stored {
            return Err(GovernanceError::NotGuardian);
        }

        let mut proposal = load_proposal(&env, proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Queued {
            return Err(GovernanceError::ProposalNotQueued);
        }

        proposal.status = ProposalStatus::Vetoed;
        save_proposal(&env, &proposal);

        env.events().publish(
            (Symbol::new(&env, "ProposalVetoed"), proposal_id),
            guardian,
        );

        Ok(())
    }

    /// Returns the full proposal struct.
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, GovernanceError> {
        load_proposal(&env, proposal_id).ok_or(GovernanceError::ProposalNotFound)
    }

    /// Returns the total number of proposals ever created.
    pub fn proposal_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, PROPOSAL_COUNT_KEY))
            .unwrap_or(0u64)
    }

    /// Updates the guardian address. Only admin may call this.
    pub fn set_guardian(env: Env, new_guardian: Address) -> Result<(), GovernanceError> {
        check_admin(&env);
        env.storage().instance().set(&Symbol::new(&env, GUARDIAN_KEY), &new_guardian);
        Ok(())
    }

    /// Updates the proposal threshold. Only admin may call this.
    pub fn set_proposal_threshold(env: Env, threshold: u64) -> Result<(), GovernanceError> {
        check_admin(&env);
        env.storage().instance().set(&Symbol::new(&env, PROPOSAL_THRESHOLD_KEY), &threshold);
        Ok(())
    }

    /// Updates the quorum. Only admin may call this.
    pub fn set_quorum(env: Env, quorum: u64) -> Result<(), GovernanceError> {
        check_admin(&env);
        env.storage().instance().set(&Symbol::new(&env, QUORUM_KEY), &quorum);
        Ok(())
    }

    /// Updates the voting period in seconds. Only admin may call this.
    pub fn set_voting_period(env: Env, seconds: u64) -> Result<(), GovernanceError> {
        check_admin(&env);
        if seconds == 0 {
            return Err(GovernanceError::InvalidParam);
        }
        env.storage().instance().set(&Symbol::new(&env, VOTING_PERIOD_KEY), &seconds);
        Ok(())
    }

    /// Updates the timelock period in seconds. Only admin may call this.
    pub fn set_timelock_period(env: Env, seconds: u64) -> Result<(), GovernanceError> {
        check_admin(&env);
        if seconds == 0 {
            return Err(GovernanceError::InvalidParam);
        }
        env.storage().instance().set(&Symbol::new(&env, TIMELOCK_PERIOD_KEY), &seconds);
        Ok(())
    }
}
