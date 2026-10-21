#![no_std]
//! # SoroStream MultiSig Contract
//!
//! M-of-N multi-signature authorization for critical administrative operations.
//!
//! ## Flow
//! 1. Any owner calls `propose` to create a pending intent, receiving an `intent_id`.
//! 2. Each owner calls `approve(intent_id)` with their auth. The SDK's `require_auth`
//!    enforces that the caller is who they claim to be.
//! 3. When M approvals are collected the operation executes automatically.
//! 4. The intent's proposer can `cancel` it before it reaches the threshold.
//!    Other owners cannot unilaterally cancel an intent they did not propose.
//!
//! ## Owner management
//! `add_owner`, `remove_owner`, and `change_threshold` are themselves guarded: callers
//! must route them through `propose` + `approve` so the multi-sig approves its own
//! configuration changes.

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, Env, Symbol, Vec,
};

// ── Storage keys ──────────────────────────────────────────────────────────────

const THRESHOLD_KEY: &str = "threshold";
const OWNER_COUNT_KEY: &str = "o_cnt";
const INTENT_COUNT_KEY: &str = "i_cnt";
const NONCE_KEY: &str = "nonce";
const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const PROPOSER_INTENTS_KEY: &str = "pi";
const TARGET_INTENTS_KEY: &str = "ti";

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntentStatus {
    Pending,
    Executed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Intent {
    pub id: u64,
    /// On-chain nonce at creation time; prevents replaying the same intent twice.
    pub nonce: u64,
    pub proposer: Address,
    /// Target contract address for execution.
    pub target: Address,
    /// Function name to call on the target.
    pub function: Symbol,
    /// ABI-encoded arguments passed verbatim to the target.
    pub calldata: Bytes,
    pub status: IntentStatus,
    /// Ledger timestamp after which the intent can no longer be approved.
    pub expiry: u64,
    /// Number of approvals collected so far.
    pub approval_count: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MultiSigError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotOwner = 3,
    IntentNotFound = 4,
    IntentNotPending = 5,
    AlreadyApproved = 6,
    IntentExpired = 7,
    InvalidThreshold = 8,
    OwnerAlreadyExists = 9,
    OwnerNotFound = 10,
    ThresholdExceedsOwners = 11,
    DuplicateOwner = 12,
    NotProposer = 13,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn owner_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "own"), addr.clone())
}

fn owner_slot_key(env: &Env, idx: u32) -> (Symbol, u32) {
    (Symbol::new(env, "oslot"), idx)
}

fn is_owner(env: &Env, addr: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&owner_key(env, addr))
        .unwrap_or(false)
}

fn add_owner_storage(env: &Env, addr: &Address) {
    let cnt: u32 = env
        .storage()
        .instance()
        .get(&Symbol::new(env, OWNER_COUNT_KEY))
        .unwrap_or(0);
    env.storage().persistent().set(&owner_key(env, addr), &true);
    env.storage()
        .persistent()
        .set(&owner_slot_key(env, cnt), addr);
    env.storage()
        .instance()
        .set(&Symbol::new(env, OWNER_COUNT_KEY), &(cnt + 1));
}

fn remove_owner_storage(env: &Env, addr: &Address) {
    let cnt: u32 = env
        .storage()
        .instance()
        .get(&Symbol::new(env, OWNER_COUNT_KEY))
        .unwrap_or(0);
    env.storage().persistent().remove(&owner_key(env, addr));
    // Compact the slot array (swap-and-pop)
    for i in 0..cnt {
        let slot: Option<Address> = env.storage().persistent().get(&owner_slot_key(env, i));
        if slot.as_ref() == Some(addr) {
            let last = cnt - 1;
            if i != last {
                let last_addr: Address = env
                    .storage()
                    .persistent()
                    .get(&owner_slot_key(env, last))
                    .unwrap();
                env.storage()
                    .persistent()
                    .set(&owner_slot_key(env, i), &last_addr);
            }
            env.storage()
                .persistent()
                .remove(&owner_slot_key(env, last));
            env.storage()
                .instance()
                .set(&Symbol::new(env, OWNER_COUNT_KEY), &last);
            return;
        }
    }
}

fn owner_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, OWNER_COUNT_KEY))
        .unwrap_or(0)
}

/// Returns every intent id proposed by `proposer` (any status).
fn proposer_intents_key(env: &Env, proposer: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "pi"), proposer.clone())
}

/// Returns every intent id targeting `target` (any status).
fn target_intents_key(env: &Env, target: &Address) -> (Symbol, Address) {
    (Symbol::new(env, "ti"), target.clone())
}

fn append_intent_index(env: &Env, key: (Symbol, Address), id: u64) {
    let mut list: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    list.push_back(id);
    env.storage().persistent().set(&key, &list);
}

fn get_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, THRESHOLD_KEY))
        .unwrap_or(1)
}

fn get_and_increment_nonce(env: &Env) -> u64 {
    let key = Symbol::new(env, NONCE_KEY);
    let nonce: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    env.storage().instance().set(&key, &(nonce + 1));
    nonce
}

fn intent_key(id: u64) -> u64 {
    id
}

fn save_intent(env: &Env, intent: &Intent) {
    env.storage()
        .persistent()
        .set(&intent_key(intent.id), intent);
}

fn load_intent(env: &Env, id: u64) -> Option<Intent> {
    env.storage().persistent().get(&intent_key(id))
}

fn next_intent_id(env: &Env) -> u64 {
    let key = Symbol::new(env, INTENT_COUNT_KEY);
    let id: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    env.storage().instance().set(&key, &(id + 1));
    id
}

fn approval_key(env: &Env, intent_id: u64, owner: &Address) -> (Symbol, u64, Address) {
    (Symbol::new(env, "appr"), intent_id, owner.clone())
}

fn has_approved(env: &Env, intent_id: u64, owner: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&approval_key(env, intent_id, owner))
        .unwrap_or(false)
}

fn mark_approved(env: &Env, intent_id: u64, owner: &Address) {
    env.storage()
        .persistent()
        .set(&approval_key(env, intent_id, owner), &true);
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, ADMIN_KEY))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct MultiSigContract;

#[contractimpl]
impl MultiSigContract {
    /// Initialises the multi-sig contract with an owner set and a threshold.
    ///
    /// `threshold` must be >= 1 and <= `owners.len()`.
    pub fn initialize(env: Env, owners: Vec<Address>, threshold: u32) -> Result<(), MultiSigError> {
        if env
            .storage()
            .instance()
            .get::<Symbol, bool>(&Symbol::new(&env, "init"))
            .unwrap_or(false)
        {
            return Err(MultiSigError::AlreadyInitialized);
        }
        if owners.is_empty() || threshold == 0 || threshold > owners.len() {
            return Err(MultiSigError::InvalidThreshold);
        }

        // Detect duplicates
        for i in 0..owners.len() {
            for j in (i + 1)..owners.len() {
                if owners.get_unchecked(i) == owners.get_unchecked(j) {
                    return Err(MultiSigError::DuplicateOwner);
                }
            }
        }

        for owner in owners.iter() {
            add_owner_storage(&env, &owner);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, THRESHOLD_KEY), &threshold);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "init"), &true);

        env.events().publish(
            (Symbol::new(&env, "MultiSigInitialized"),),
            (owners, threshold),
        );
        Ok(())
    }

    /// Creates a new pending intent. Any owner may propose.
    ///
    /// Returns the `intent_id` to share with other owners for approval.
    /// `expiry` is a ledger timestamp after which the intent can no longer be executed.
    pub fn propose(
        env: Env,
        proposer: Address,
        target: Address,
        function: Symbol,
        calldata: Bytes,
        expiry: u64,
    ) -> Result<u64, MultiSigError> {
        proposer.require_auth();
        if !is_owner(&env, &proposer) {
            return Err(MultiSigError::NotOwner);
        }

        let nonce = get_and_increment_nonce(&env);
        let id = next_intent_id(&env);

        let intent = Intent {
            id,
            nonce,
            proposer: proposer.clone(),
            target: target.clone(),
            function: function.clone(),
            calldata,
            status: IntentStatus::Pending,
            expiry,
            approval_count: 0,
        };
        save_intent(&env, &intent);

        env.events().publish(
            (Symbol::new(&env, "IntentProposed"), id),
            (proposer, target, function, nonce, expiry, calldata),
        );

        Ok(id)
    }

    /// Approves a pending intent. Each owner may approve only once.
    ///
    /// When the approval count reaches the threshold the operation executes
    /// automatically and `true` is returned. Returns `false` if the threshold
    /// has not yet been reached.
    ///
    /// Execution invokes `intent.function` on `intent.target` with
    /// `intent.calldata` as a single raw `Bytes` argument, assuming the target
    /// returns `()`. This is the generic "invoke a target with one raw `Bytes`
    /// argument" execution convention shared with governance's `execute`.
    ///
    /// It is NOT the convention used by the main stream contract, whose entry
    /// points take distinct typed parameters and return `Result<_, StreamError>`.
    /// Consequently, a multi-sig intent cannot directly invoke the stream
    /// contract's real entry points through this path; an adapter accepting a
    /// single `Bytes` argument would be required.
    pub fn approve(env: Env, owner: Address, intent_id: u64) -> Result<bool, MultiSigError> {
        owner.require_auth();
        if !is_owner(&env, &owner) {
            return Err(MultiSigError::NotOwner);
        }

        let mut intent = load_intent(&env, intent_id).ok_or(MultiSigError::IntentNotFound)?;

        if intent.status != IntentStatus::Pending {
            return Err(MultiSigError::IntentNotPending);
        }

        let now = env.ledger().timestamp();
        if now > intent.expiry {
            return Err(MultiSigError::IntentExpired);
        }

        if has_approved(&env, intent_id, &owner) {
            return Err(MultiSigError::AlreadyApproved);
        }

        mark_approved(&env, intent_id, &owner);
        intent.approval_count += 1;

        env.events().publish(
            (Symbol::new(&env, "SignatureSubmitted"), intent_id),
            (owner, intent.approval_count),
        );

        let threshold = get_threshold(&env);

        if intent.approval_count >= threshold {
            intent.status = IntentStatus::Executed;
            save_intent(&env, &intent);

            // Secondary indexes so intents can be listed by proposer or target.
            append_intent_index(&env, proposer_intents_key(&env, &proposer), id);
            append_intent_index(&env, target_intents_key(&env, &target), id);

            env.events().publish(
                (Symbol::new(&env, "ThresholdReached"), intent_id),
                threshold,
            );

            env.invoke_contract::<()>(
                &intent.target,
                &intent.function,
                Vec::from_array(
                    &env,
                    [soroban_sdk::IntoVal::into_val(&intent.calldata, &env)],
                ),
            );

            env.events()
                .publish((Symbol::new(&env, "ExecutionCompleted"), intent_id), ());

            Ok(true)
        } else {
            save_intent(&env, &intent);
            Ok(false)
        }
    }

    /// Cancels a pending intent. Only the original proposer may cancel;
    /// other owners must not be able to unilaterally kill an intent they
    /// did not propose (e.g. one already backed by several approvals).
    pub fn cancel(env: Env, owner: Address, intent_id: u64) -> Result<(), MultiSigError> {
        owner.require_auth();
        if !is_owner(&env, &owner) {
            return Err(MultiSigError::NotOwner);
        }

        let mut intent = load_intent(&env, intent_id).ok_or(MultiSigError::IntentNotFound)?;

        if intent.status != IntentStatus::Pending {
            return Err(MultiSigError::IntentNotPending);
        }

        if owner != intent.proposer {
            return Err(MultiSigError::NotProposer);
        }

        intent.status = IntentStatus::Cancelled;
        save_intent(&env, &intent);

        env.events()
            .publish((Symbol::new(&env, "IntentCancelled"), intent_id), owner);

        Ok(())
    }

    /// Adds a new owner. Must be called via the multi-sig (propose + approve).
    pub fn add_owner(env: Env, new_owner: Address) -> Result<(), MultiSigError> {
        env.current_contract_address().require_auth();
        if is_owner(&env, &new_owner) {
            return Err(MultiSigError::OwnerAlreadyExists);
        }
        add_owner_storage(&env, &new_owner);
        env.events()
            .publish((Symbol::new(&env, "OwnerAdded"),), new_owner);
        Ok(())
    }

    /// Removes an existing owner. Must be called via the multi-sig (propose + approve).
    ///
    /// The threshold must remain reachable after removal.
    pub fn remove_owner(env: Env, owner: Address) -> Result<(), MultiSigError> {
        env.current_contract_address().require_auth();
        if !is_owner(&env, &owner) {
            return Err(MultiSigError::OwnerNotFound);
        }
        let new_count = owner_count(&env).saturating_sub(1);
        if get_threshold(&env) > new_count {
            return Err(MultiSigError::ThresholdExceedsOwners);
        }
        remove_owner_storage(&env, &owner);
        env.events()
            .publish((Symbol::new(&env, "OwnerRemoved"),), owner);
        Ok(())
    }

    /// Changes the approval threshold. Must be called via the multi-sig (propose + approve).
    pub fn change_threshold(env: Env, new_threshold: u32) -> Result<(), MultiSigError> {
        env.current_contract_address().require_auth();
        if new_threshold == 0 || new_threshold > owner_count(&env) {
            return Err(MultiSigError::InvalidThreshold);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, THRESHOLD_KEY), &new_threshold);
        env.events()
            .publish((Symbol::new(&env, "ThresholdChanged"),), new_threshold);
        Ok(())
    }

    /// Returns whether an address is an owner.
    pub fn is_owner(env: Env, addr: Address) -> bool {
        is_owner(&env, &addr)
    }

    /// Returns the current threshold.
    pub fn get_threshold(env: Env) -> u32 {
        get_threshold(&env)
    }

    /// Returns the current owner count.
    pub fn owner_count(env: Env) -> u32 {
        owner_count(&env)
    }

    /// Returns the full list of current owner addresses.
    pub fn get_owners(env: Env) -> Vec<Address> {
        let cnt = owner_count(&env);
        let mut owners = Vec::new(&env);
        for i in 0..cnt {
            if let Some(addr) = env.storage().persistent().get(&owner_slot_key(&env, i)) {
                owners.push_back(addr);
            }
        }
        owners
    }

    /// Returns the current global nonce (next value that will be used).
    pub fn get_nonce(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, NONCE_KEY))
            .unwrap_or(0u64)
    }

    /// Returns the full intent struct.
    pub fn get_intent(env: Env, intent_id: u64) -> Result<Intent, MultiSigError> {
        load_intent(&env, intent_id).ok_or(MultiSigError::IntentNotFound)
    }

    /// Returns the ids of all intents proposed by `proposer` (any status).
    ///
    /// Use [`get_intent`] to inspect each intent's status and filter for
    /// `IntentStatus::Pending` if only live intents are needed.
    pub fn get_intents_by_proposer(env: Env, proposer: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&proposer_intents_key(&env, &proposer))
            .unwrap_or(Vec::new(&env))
    }

    /// Returns the ids of all intents targeting `target` (any status).
    ///
    /// Use [`get_intent`] to inspect each intent's status and filter for
    /// `IntentStatus::Pending` if only live intents are needed.
    pub fn get_intents_by_target(env: Env, target: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&target_intents_key(&env, &target))
            .unwrap_or(Vec::new(&env))
    }

    /// Returns whether an owner has already approved a given intent.
    pub fn has_approved(env: Env, intent_id: u64, owner: Address) -> bool {
        has_approved(&env, intent_id, &owner)
    }

    /// Returns the current admin address, if set.
    pub fn get_admin(env: Env) -> Option<Address> {
        read_admin(&env)
    }

    /// Sets the admin. Only the multi-sig contract itself may call this (via propose+approve).
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), MultiSigError> {
        env.current_contract_address().require_auth();
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &new_admin);
        env.events()
            .publish((Symbol::new(&env, "AdminSet"),), new_admin);
        Ok(())
    }

    /// Proposes a new admin. Only an owner may propose (through propose+approve).
    pub fn propose_transfer_admin(env: Env, new_admin: Address) -> Result<u64, MultiSigError> {
        env.current_contract_address().require_auth();
        let nonce = get_and_increment_nonce(&env);
        let id = next_intent_id(&env);

        let intent = Intent {
            id,
            nonce,
            proposer: env.current_contract_address(),
            target: env.current_contract_address(),
            function: Symbol::new(&env, "accept_transfer_admin"),
            calldata: Bytes::new(&env),
            status: IntentStatus::Pending,
            expiry: u64::MAX,
            approval_count: 0,
        };
        save_intent(&env, &intent);

        // Store the proposed admin separately
        env.storage()
            .instance()
            .set(&Symbol::new(&env, PENDING_ADMIN_KEY), &new_admin);

        env.events()
            .publish((Symbol::new(&env, "AdminTransferProposed"), id), new_admin);

        Ok(id)
    }

    /// Accepts the admin role after proposal has been approved by the multi-sig.
    /// Called internally by the approve process.
    pub fn accept_transfer_admin(env: Env) -> Result<(), MultiSigError> {
        env.current_contract_address().require_auth();
        let pending = read_pending_admin(&env).ok_or(MultiSigError::OwnerNotFound)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, ADMIN_KEY), &pending);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        env.events()
            .publish((Symbol::new(&env, "AdminTransferAccepted"),), pending);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

    fn setup() -> (Env, Address, Vec<Address>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MultiSigContract, ());
        let owner1 = Address::generate(&env);
        let owner2 = Address::generate(&env);
        let owner3 = Address::generate(&env);
        let owners = vec![&env, owner1.clone(), owner2.clone(), owner3.clone()];
        let c = MultiSigContractClient::new(&env, &contract_id);
        assert!(c.initialize(&owners, &2u32).is_ok());
        (env, contract_id, vec![owner1, owner2, owner3])
    }

    #[test]
    fn test_approve_after_expiry_rejected_with_prior_approvals() {
        let (env, contract_id, owners) = setup();
        let c = MultiSigContractClient::new(&env, &contract_id);
        let owner1 = owners[0].clone();
        let owner2 = owners[1].clone();
        let target = Address::generate(&env);
        let function = Symbol::new(&env, "dummy");
        let calldata = Bytes::new(&env);

        env.ledger().set_timestamp(1000);
        let intent_id = c
            .propose(&owner1, &target, &function, &calldata, &2000u64)
            .unwrap();

        // owner1 approves before the expiry
        assert!(c.approve(&owner1, &intent_id).unwrap());

        // advance time past the intent's expiry
        env.ledger().set_timestamp(3000);

        // owner2 attempts to approve after expiry: must be rejected even though
        // the threshold has not been reached and a prior approval exists.
        let result = c.try_approve(&owner2, &intent_id);
        assert_eq!(result, Err(Ok(MultiSigError::IntentExpired)));

        // the intent is still pending with only the single prior approval
        let intent = c.get_intent(&intent_id).unwrap();
        assert_eq!(intent.status, IntentStatus::Pending);
        assert_eq!(intent.approval_count, 1);
    }
}
