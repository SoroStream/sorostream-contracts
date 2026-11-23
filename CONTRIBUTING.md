# Contributing to sorostream-contracts

Thank you for your interest in contributing to SoroStream! This repo participates in the **Stellar Wave Program** on [Drips Wave](https://drips.network/wave).

## Wave Contributor Workflow

1. **Browse open issues** — find one labelled `Stellar Wave` with a complexity you're comfortable with.
2. **Apply via Drips Wave** — do **not** begin coding until the maintainer assigns you to the issue.
3. **Fork the repo** and create a branch:
   - Bug fixes: `fix/N-short-description`
   - Features: `feat/N-short-description`
   - Where `N` is the issue number (e.g. `feat/4-pagination`).
4. **Write code and tests** — `cargo test` and `cargo clippy -- -D warnings` must pass.
5. **Open a PR** — the title must reference the issue (e.g. `feat: add pagination (#4)`), and the body must include `Closes #N`.
6. **Await review** — the maintainer will review and merge. Once merged and the issue is resolved before the Wave ends, you earn your Points.

## Local Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASM target
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install --locked stellar-cli --features opt

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Build contract WASM
stellar contract build
```

## Security Audit Checklist

Every PR that adds or modifies a contract instruction **must** pass through this checklist. Copy it into your PR description (the PR template includes it automatically).

### Input Validation

- [ ] All numeric inputs are checked for zero, negative, or overflow conditions before use.
  *Example:* [`create_stream` checks `amount <= 0`](./contracts/stream/src/lib.rs) and [`cliff_seconds > duration_seconds`](./contracts/stream/src/lib.rs).
- [ ] Vector/array inputs are bounds-checked (matching lengths, capped size).
  *Example:* [`batch_create_stream` checks `recipients.len() != amounts.len()`](./contracts/stream/src/lib.rs).

### Authorization Checks

- [ ] Every state-mutating function calls `require_auth()` on the appropriate party (sender, recipient, or admin).
  *Example:* [`withdraw` calls `recipient.require_auth()`](./contracts/stream/src/lib.rs).
- [ ] Admin-only functions use `check_admin()` which reads and verifies the stored admin address.
  *Example:* [`pause` calls `check_admin(&env)`](./contracts/stream/src/lib.rs).
- [ ] No unauthorized address can modify another user's streams or funds.

### Arithmetic Overflow

- [ ] All arithmetic uses `saturating_sub`, `checked_add`, or equivalent — never raw subtraction that could underflow.
  *Example:* [`cancel_stream` uses `stream.deposit.saturating_sub(...)`](./contracts/stream/src/lib.rs).
- [ ] Division-before-multiplication patterns are avoided (or documented if intentional for dust handling).
  *Example:* [`top_up` calculates `effective_amount` to discard sub-flow-rate dust](./contracts/stream/src/lib.rs).

### Storage Cleanup

- [ ] Completed or removed streams are cleaned up from persistent storage.
  *Example:* [`withdraw` calls `remove_stream` when a non-renewing stream completes](./contracts/stream/src/lib.rs).
- [ ] New storage keys are documented in [`docs/STORAGE.md`](./docs/STORAGE.md).
- [ ] Indexes and canonical records use the same durability level (see [STORAGE.md](./docs/STORAGE.md) for why).

### Event Emission

- [ ] Every state change emits an event so off-chain indexers can track it.
  *Example:* [`create_stream` emits `StreamCreated`](./contracts/stream/src/events.rs); [`cancel_stream` emits `StreamCancelled`](./contracts/stream/src/events.rs).
- [ ] Events include enough data for an indexer to reconstruct state without querying the contract.

### Property-Based Testing Guidance

When adding a new instruction, write property-based tests that verify invariants hold across random inputs. See the [Property-Based Testing](#property-based-testing) section below for a full guide.

## Code Style

- Follow standard Rust formatting (`cargo fmt`).
- All public functions must have doc comments.
- No `unwrap()` in contract code — use `Result` with `StreamError`.

## Contract Storage (read before touching `storage.rs`)

Soroban contracts have **instance**, **persistent**, and **temporary** storage. They differ in cost, TTL behavior, and — critically — what happens when entries expire. Using the wrong type causes **silent data loss**: for example, storing stream indexes in temporary storage while streams live in persistent storage makes `get_streams_by_sender` return empty results even though streams still exist ([#1](https://github.com/SoroStream/sorostream-contracts/issues/1)).

**Before adding or changing contract state:**

1. Read [docs/STORAGE.md](./docs/STORAGE.md) for the full trade-off guide and the current key layout in `contracts/stream/src/storage.rs`.
2. Never put long-lived indexes, balances, or user-visible state in `env.storage().temporary()` without maintainer approval and an explicit TTL-extension plan.
3. Keep canonical records and their lookup indexes on the **same** durability level (today: persistent for streams + sender/recipient slots + nonces; instance for admin/pause/counter/fees).
4. Update `docs/STORAGE.md` when you introduce new storage keys so the next contributor does not repeat past mistakes.

## Property-Based Testing

Property-based testing (PBT) runs your assertions against hundreds or thousands of randomly generated inputs rather than a single hand-crafted example. This makes it far more effective at finding edge cases in arithmetic-heavy contract logic like flow-rate calculations, dust handling, and balance conservation.

SoroStream uses [`proptest`](https://proptest-rs.github.io/proptest/) with the Soroban test environment. All property tests live in `contracts/stream/src/proptest_tests.rs`.

---

### Setup

`proptest` is already a dev-dependency:

```toml
# contracts/stream/Cargo.toml
[dev-dependencies]
proptest = "1"
```

No additional installation is needed. Ensure the `testutils` feature is available — it is enabled automatically when running `cargo test`.

---

### Running Property Tests

```bash
# Run all property tests (default: 256 cases per property)
cargo test proptest_tests -- --nocapture

# Run with a higher case count (matches CI)
PROPTEST_CASES=1000 cargo test proptest_tests -- --nocapture

# Run a single named property
cargo test proptest_tests::prop_cancel_refund_invariant -- --nocapture
```

The CI `property-tests` job runs with `PROPTEST_CASES=1000`. Locally you can use fewer cases for faster iteration.

---

### Defining Generators

Generators describe the shape of random inputs. Use `proptest`'s built-in range strategies for scalar values, and compose them with `prop_map` or `prop_flat_map` for derived values.

```rust
// Simple scalar ranges
proptest! {
    #[test]
    fn prop_example(
        amount   in 1_000_i128..=10_000_000_i128,
        duration in 10_u64..=100_000_u64,
        t_offset in 0_u64..=100_000_u64,
    ) {
        // proptest generates random (amount, duration, t_offset) tuples
        // and runs the body repeatedly
    }
}
```

For parameters that are **derived** from others (e.g. a valid cliff that must be ≤ duration), clamp or skip inside the test body rather than using complex strategies — this keeps the generator simple and the shrinking fast:

```rust
proptest! {
    #[test]
    fn prop_cliff_within_duration(
        amount   in 1_000_i128..=1_000_000_i128,
        duration in 100_u64..=10_000_u64,
        cliff    in 0_u64..=10_000_u64,
    ) {
        // Clamp cliff to be valid — avoids InvalidCliff errors
        let cliff = cliff.min(duration);
        // ... rest of test
    }
}
```

To **skip** a generated input that would be trivially invalid (e.g. flow_rate would be zero), use an early `return Ok(())`:

```rust
let flow_rate = amount / duration as i128;
if flow_rate == 0 {
    return Ok(()); // skip — ZeroFlowRate would be a contract-level error, not a bug
}
```

---

### Assertion Patterns

Structure each property test around a single invariant. Keep setup minimal and the assertion at the bottom clear.

**Pattern 1 — Balance conservation:**

```rust
prop_assert_eq!(
    sender_before - sender_after,
    amount,
    "sender must lose exactly `amount` tokens on create"
);
prop_assert_eq!(
    contract_after - contract_before,
    amount,
    "contract must gain exactly `amount` tokens on create"
);
```

**Pattern 2 — Monotonic bounds:**

```rust
prop_assert!(
    bal_after >= bal_before,
    "recipient balance must be non-decreasing across withdrawals"
);
```

**Pattern 3 — Field consistency:**

```rust
let stream = client.get_stream(&stream_id);
prop_assert_eq!(stream.deposit, amount);
prop_assert_eq!(stream.flow_rate, amount / duration as i128);
prop_assert_eq!(stream.end_time, start_time + duration);
```

**Pattern 4 — Total conservation after cancel:**

```rust
let refund    = sender_after   - sender_before_cancel;
let earned    = recipient_after - recipient_before_cancel;
prop_assert_eq!(
    refund + earned,
    deposit,
    "cancel must return exactly the full deposit split between sender and recipient"
);
```

---

### Worked Example — Cancel Refund Invariant

This example verifies the core cancel invariant: `sender_refund + recipient_earned == deposit` for any valid `(amount, duration, cancel_time)` triple.

```rust
// contracts/stream/src/proptest_tests.rs

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Cancel refund invariant: the sum of refund and earned always equals the deposit.
    #[test]
    fn prop_cancel_refund_invariant(
        amount      in 10_000_i128..=1_000_000_i128,
        duration    in 100_u64..=10_000_u64,
        cancel_time in 0_u64..=10_000_u64,
    ) {
        // ── setup ────────────────────────────────────────────────────────────
        let (env, contract_id, token_id, sender, recipient) = setup_env();
        let client = SoroStreamContractClient::new(&env, &contract_id);
        let token  = TokenClient::new(&env, &token_id);

        // Skip degenerate inputs where no tokens would ever flow
        let flow_rate = amount / duration as i128;
        if flow_rate == 0 {
            return Ok(());
        }

        env.ledger().set_timestamp(0);

        // ── create stream ────────────────────────────────────────────────────
        let stream_id = client.create_stream(
            &sender,
            &recipient,
            &token_id,
            &amount,
            &duration,
            &0u64,   // cliff
            &0u64,   // lock_until
            &false,  // auto_renew
            &0u64,   // start_time (0 = use ledger timestamp)
            &false,  // allow_recipient_termination
            &0i128,  // holdback_amount
        );

        // Record contract balance right after creation
        let contract_bal_after_create = token.balance(&contract_id);
        prop_assert_eq!(contract_bal_after_create, amount,
            "contract must hold exactly `amount` after create");

        // ── advance time and cancel ──────────────────────────────────────────
        let cancel_time = cancel_time.min(duration); // keep within stream lifetime
        env.ledger().set_timestamp(cancel_time);

        let sender_before_cancel    = token.balance(&sender);
        let recipient_before_cancel = token.balance(&recipient);

        client.cancel_stream(&stream_id, &sender);

        let sender_refund = token.balance(&sender)    - sender_before_cancel;
        let earned        = token.balance(&recipient) - recipient_before_cancel;

        // ── assert invariant ─────────────────────────────────────────────────
        prop_assert_eq!(
            sender_refund + earned,
            amount,
            "sender_refund ({sender_refund}) + earned ({earned}) must equal deposit ({amount})"
        );

        // Neither party receives more than the deposit
        prop_assert!(sender_refund >= 0,  "refund must be non-negative");
        prop_assert!(earned        >= 0,  "earned must be non-negative");
        prop_assert!(sender_refund <= amount, "refund must not exceed deposit");
        prop_assert!(earned        <= amount, "earned must not exceed deposit");
    }
}
```

Run it:

```bash
PROPTEST_CASES=10000 cargo test prop_cancel_refund_invariant -- --nocapture
```

---

### Debugging Shrunk Counter-Examples

When `proptest` finds a failing input it automatically **shrinks** it — repeatedly simplifying the values until it finds the smallest input that still fails. The shrunk counter-example is printed at the end of the test output:

```
thread 'proptest_tests::prop_cancel_refund_invariant' panicked at
  'prop_cancel_refund_invariant: ...
   Minimal failing input:
     amount = 10000
     duration = 100
     cancel_time = 0
   Explanation: sender_refund (0) + earned (0) must equal deposit (10000)'
```

**Steps to debug a shrunk counter-example:**

1. **Reproduce it in a unit test.** Copy the printed values into a `#[test]` with exact inputs — this is faster to iterate on than re-running proptest.

   ```rust
   #[test]
   fn regression_cancel_zero_time() {
       // Reproduces: amount=10000, duration=100, cancel_time=0
       let (env, contract_id, token_id, sender, recipient) = setup_env();
       // ...
   }
   ```

2. **Check the contract logic path** for the shrunk values. Zero `cancel_time` means `elapsed = 0`, so `earned = flow_rate × 0 = 0`. If the contract still transfers tokens, something is wrong in the claimable calculation.

3. **Add `dbg!()` or `println!()` calls** in the test body, then re-run with `--nocapture` to inspect intermediate state.

4. **Use the proptest seed** to re-run the exact random sequence. The seed is printed in the failure message. Set it via:

   ```bash
   PROPTEST_SEED="<hex-seed>" cargo test prop_cancel_refund_invariant
   ```

5. **Persist the failure case** by saving it to `proptest-regressions/`. Proptest saves shrunk cases automatically in `contracts/stream/proptest-regressions/` if the directory exists — commit this file to prevent regressions.

---

### Invariants to Cover for New Instructions

When you add a new contract instruction, consider writing property tests for these classes of invariant:

| Invariant class | What to assert |
|----------------|----------------|
| **Balance conservation** | Tokens in = tokens out; no tokens created or destroyed |
| **Monotonic time** | `last_withdraw_time` never decreases; `end_time` only increases on `top_up` |
| **Bounded withdrawal** | Total withdrawn never exceeds deposit |
| **Field consistency** | `flow_rate == deposit / duration`; `end_time == start_time + duration` |
| **Index consistency** | `get_streams_by_sender` returns every stream created by that sender |
| **Idempotency guards** | Same nonce always returns `DuplicateStream`; double-cancel fails |
| **Status transitions** | Cancelled / completed streams cannot be re-cancelled or withdrawn from |

> Closes [#318](https://github.com/SoroStream/sorostream-contracts/issues/318).

---

## Storage Layout Snapshots

Soroban contract storage is binary-encoded XDR. A schema change — added field, renamed variant, type change — can silently break deployed state reads without any compile-time error. Committed storage snapshots make this visible at review time.

### How it works

The soroban-sdk `testutils` infrastructure automatically writes a JSON file at the end of every `#[test]` that uses `Env::default()`. The file captures the full ledger state (all storage entries + events) in the same binary-equivalent JSON representation that the Soroban host uses on-chain. Files are placed at:

```
contracts/stream/test_snapshots/test/<test_name>.1.json
```

The four lifecycle scenario snapshots (`storage_snapshot_create`, `storage_snapshot_withdraw`, `storage_snapshot_cancel`, `storage_snapshot_top_up`) are committed to the repository. CI runs them as part of the `integration` job and then checks `git diff --exit-code contracts/stream/test_snapshots/` to ensure no JSON was generated but not staged.

If the binary encoding of any `#[contracttype]` changes, the test will regenerate a different JSON file, the `git diff` check will fail, and the PR will be blocked until the author explicitly regenerates and commits the updated snapshots.

### Normal development — no schema change

Nothing special required. Run `cargo test` as usual. If the snapshot tests pass and produce no diff, you are done.

### Updating snapshots after an intentional schema change

When you intentionally change a `#[contracttype]` (add a field, rename a variant, change a type), you must regenerate the committed snapshots:

```bash
# 1. Run the snapshot tests with the update flag set.
#    The SDK overwrites test_snapshots/test/storage_snapshot_*.1.json in place.
UPDATE_EXPECT=true cargo test --package sorostream-stream \
    -- storage_layout_snapshot_tests --nocapture

# 2. Review the diff carefully — every changed field should be expected.
git diff contracts/stream/test_snapshots/

# 3. Stage and commit the updated snapshots as part of the same PR.
git add contracts/stream/test_snapshots/test/storage_snapshot_*.1.json
git commit -m "chore: regenerate storage layout snapshots after schema change"
```

> **Rule:** A PR that changes a `#[contracttype]` or adds a new storage key **must** include the regenerated snapshots in the same commit. Reviewers use the snapshot diff to verify the on-chain format change is intentional.

### Adding new snapshot scenarios

If you add a new entry point that writes a new storage key, add a corresponding `#[test]` to `contracts/stream/src/storage_layout_snapshot_tests.rs` that exercises the entry point. Run with `UPDATE_EXPECT=true` once to generate the baseline, then commit both the test and the snapshot file.

### Storage durability guidelines (recap)

Before adding or changing any storage key, re-read the durability rules in [docs/STORAGE.md](./docs/STORAGE.md) and the key-layout table in [ARCHITECTURE.md](./ARCHITECTURE.md). Wrong durability (e.g. putting a long-lived index in `temporary`) causes silent data loss. Add every new key to both documents.
