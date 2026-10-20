# Technical Analysis: MultiSig Self-Targeting Proposal Invariant Audit (#838)

## 1. Problem Statement & Scope

Issue #838 raises a critical security question regarding `MultiSigContract` in `contracts/multisig/src/lib.rs`:

> Does `propose()` need to validate that `target != env.current_contract_address()`? Specifically, could a self-targeting intent executed via `propose` + `approve` bypass the invariants enforced by dedicated administrative entry points (`add_owner`, `remove_owner`, `change_threshold`)?

---

## 2. Invariant Analysis & Audit Findings

### A. Invariant Enforcement at the Function Level
Each administrative function in `MultiSigContract` directly requires contract authorization:
```rust
env.current_contract_address().require_auth();
```
When called directly, they enforce the following invariants:
1. **`add_owner(new_owner: Address)`**:
   - `is_owner(&env, &new_owner)` -> Rejects with `MultiSigError::OwnerAlreadyExists`.
2. **`remove_owner(owner: Address)`**:
   - `!is_owner(&env, &owner)` -> Rejects with `MultiSigError::OwnerNotFound`.
   - `get_threshold(&env) > new_count` -> Rejects with `MultiSigError::ThresholdExceedsOwners`.
3. **`change_threshold(new_threshold: u32)`**:
   - `new_threshold == 0 || new_threshold > owner_count(&env)` -> Rejects with `MultiSigError::InvalidThreshold`.

Because these checks are located directly inside the function bodies rather than in external wrappers, any call executed as the contract itself **must** pass through these assertions. There is no alternative code path inside the contract that modifies owner storage or threshold values without triggering these checks.

### B. The Single-Bytes-Argument Execution Bug (Non-Functional Path)
In `approve()`, when threshold approval is reached:
```rust
env.invoke_contract::<()>(
    &intent.target,
    &intent.function,
    Vec::from_array(&env, [soroban_sdk::IntoVal::into_val(&intent.calldata, &env)]),
);
```
`invoke_contract` hardcodes a 1-element argument vector containing `intent.calldata` as a single `Bytes` object.

When an intent targets `env.current_contract_address()`:
1. `add_owner` expects `(new_owner: Address)`.
2. `remove_owner` expects `(owner: Address)`.
3. `change_threshold` expects `(new_threshold: u32)`.

Because the host dispatch layer attempts to deserialize a `Bytes` argument into an `Address` or `u32`, the invocation aborts at the Soroban host ABI boundary before executing the function logic.

**Conclusion:** The self-targeting execution path is currently **non-functional** rather than unsafe due to this parameter encoding mismatch.

### C. Future Defense-in-Depth Recommendation
Even once dynamic calldata decoding is supported:
1. If self-targeting intents are intended to invoke `add_owner`, `remove_owner`, and `change_threshold`, all invariants remain strictly enforced because those functions perform atomic boundary checks before mutating storage.
2. If self-targeting was never intended to route through generic calldata execution, an explicit guard in `propose()` can be added for clarity:
   ```rust
   // Optional defense-in-depth guard:
   // if target == env.current_contract_address() { return Err(MultiSigError::InvalidTarget); }
   ```

---

## 3. Test Verification Matrix

Automated tests added in `contracts/multisig/src/test.rs` verify:
- `test_multisig_initialization_invariants`: Confirms initial threshold and owner counts.
- `test_direct_owner_management_enforces_invariants`: Validates that `ThresholdExceedsOwners`, `InvalidThreshold`, and `OwnerAlreadyExists` are strictly raised.
- `test_self_targeting_propose_and_single_bytes_invocation_boundary`: Verifies proposing self-target and confirms host dispatch aborts due to parameter type mismatch.
- `test_removal_boundary_preserves_threshold_safety`: Verifies threshold cannot be exceeded by owner removal.
