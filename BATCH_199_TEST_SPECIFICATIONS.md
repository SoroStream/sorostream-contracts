# Batch-199: SoroStream Test Coverage Specifications

## Issue #827: Factory propose_admin Overwriting Pending Admin

**Location:** `contracts/factory/src/test.rs`

**Test Name:** `test_propose_admin_overwrites_pending_admin`

**Pattern:** Follow `test_deploy_stores_label_and_info_roundtrips` structure

**Steps:**
1. Initialize factory with `admin`
2. Call `propose_admin(new_admin_1)` → verifies `PENDING_ADMIN_KEY = new_admin_1`
3. Call `propose_admin(new_admin_2)` BEFORE `new_admin_1` accepts
4. Verify `PENDING_ADMIN_KEY` now points to `new_admin_2` (overwritten)
5. Attempt: `new_admin_1.accept_admin()` → should fail with `Unauthorized`
6. Attempt: `new_admin_2.accept_admin()` → should succeed

**Expected:** First proposal is silently superseded; original proposer loses acceptance rights

---

## Issue #828: Treasury set_treasury_split at 0 bps Boundary

**Location:** `contracts/treasury/src/lib.rs` (or separate `test.rs` if exists)

**Test Name:** `test_distribute_at_zero_bps_treasury_split`

**Context:** `set_treasury_split` rejects if `treasury_bps > 10_000`, making `0` a valid "100% to LP pool" configuration

**Test Verification:**
- `set_treasury_split(0)` accepted
- Call `distribute(amount, token, lp_pool, destination)`
- 100% routed to `lp_pool`, 0% to `destination`
- `destination.transfer()` branch skipped (amount = 0, no panic)
- No underflow/overflow at boundary

**Expected:** Works correctly as full LP pool route without errors

---

## Issue #829: Governance Fuzz Test with i128::MAX Balance

**Location:** `contracts/governance/src/test.rs` or `governance_fuzz_tests.rs`

**Test Name:** `test_create_proposal_vote_with_token_balance_i128_max`

**Background:** `vote()` / `create_proposal()` read token balance as `u64`, truncating at `i128::MAX`

**Setup:**
- Mock token contract returning exactly `i128::MAX` balance
- Call `create_proposal()` with this token
- Call `vote()` operations

**Expected:** Documents current behavior or post-fix validation at extreme input

**Pattern:** Reference `stream/src/proptest_tests.rs` for fuzzing style

---

## Issue #830: Governance execute() Benchmark

**Location:** `contracts/governance/src/lib.rs` (new test/bench module or `governance_bench_tests.rs`)

**Benchmark Name:** `bench_execute_noop_vs_complex_call`

**Prerequisites:** Requires fix for `execute()` to support real multi-argument target calls

**Benchmarks to Create:**
1. `execute()` with no-op target call (minimal args)
2. `execute()` with complex multi-argument target call

**Measures:** Gas cost scaling with argument complexity against Soroban per-invocation limits

**Tool:** Use `criterion` benchmark pattern if available, or manual gas tracking via `env`

---

## Testing Patterns to Follow

- **Factory:** `env.register()`, `env.mock_all_auths()`, `Address::generate()`
- **Treasury:** Similar to factory pattern; verify state changes in Instance storage
- **Governance:** Reuse existing helpers: `create_proposal()`, `vote()`, `queue()` from test.rs
- **Benchmarks:** Add to existing test module or create `*_bench_tests.rs`; measure execution cost

All tests should follow existing code style and naming conventions in their respective files.
