# Chaos Testing Framework

Implementation of **issue #656** — inject random token-contract failures, ledger
resets and other Soroban-level faults, and verify that the protocol handles them
without panicking or corrupting accounting.

## What ships

| File | Purpose |
|---|---|
| `contracts/stream/src/chaos.rs` | The fault-injection framework (test-only module) |
| `contracts/stream/src/issue_656_chaos_tests.rs` | Campaigns that drive the contract under injected faults |

The module is gated behind `#[cfg(test)]`, so it has **zero** effect on the
compiled Wasm contract or on the deployed bytecode size.

## Architecture

```text
        ChaosConfig (seed, fault_rate, enabled kinds)
                    │
                    ▼
             ChaosRng (xorshift64*)  ── deterministic fault schedule
                    │
                    ▼
    ┌───────────────────────────────────────────┐
    │              ChaosEnv                      │
    │  inject(kind)  ──▶ arm the fault           │
    │  clear()       ──▶ disarm + restore budget │
    │  record_*()    ──▶ classify the outcome    │
    └───────────────────────────────────────────┘
         │                │                │
         ▼                ▼                ▼
    ChaosToken      ledger faults      ChaosReport
  (token failures)  (clock/seq/TTL/   (handled /
                     storage/budget)   recovered / finding)
```

### Three fault surfaces

1. **Token-contract failures** — [`ChaosToken`] is a minimal SEP-41-shaped mock
   whose `transfer` can be armed to reject, to report insufficient balance, or to
   abort the host invocation outright. This is the surface the issue calls out
   first, and it is the one with the most protocol coupling: a rejected refund
   must not move the contract's balance, and a rejected withdrawal must not mark
   the stream as paid.
2. **Ledger-level faults** — `ClockRewind`, `SequenceSkew`, `StreamRecordLoss`,
   `BudgetStarvation`, `SnapshotRollback` and `TrustorInsolvency` manipulate the
   host directly through `env.ledger()`, `env.as_contract()` and
   `env.cost_estimate().budget()`.
3. **The schedule** — `ChaosRng` decides *which* fault lands before *which*
   operation, reproducibly from a seed.

## Graceful degradation, defined

A step is graceful if it ends in exactly one of:

| Outcome | Meaning |
|---|---|
| `Succeeded` | the entry point returned `Ok`, no fault armed |
| `Recovered` | the entry point returned `Ok` *despite* an armed fault |
| `Handled(code)` | the entry point returned a `StreamError` declared in `DEFINED_ERROR_CODES` |

Anything else is a **finding**, and the campaign fails:

- a panic escaping the host,
- an error code outside `DEFINED_ERROR_CODES` (an *undefined* error code),
- a balance that went negative,
- a read-only operation that moved escrow,
- a stream left in an undeclared status,
- an error discriminant declared more than once.

## The `no panics with undefined error codes` criterion

`DEFINED_ERROR_CODES` is an explicit, sorted table of the 50 declared
`StreamError` discriminants, paired index-for-index with `ERROR_CODE_NAMES`.

Keeping the table explicit rather than deriving it from the enum is deliberate.
Deriving it would make the check vacuous: it would only ever compare the enum
against itself. An explicit table is an independent restatement of the contract,
so an entry point that returns an undeclared code — or a variant added without
updating the table — is caught.

Two additional guards run against the table itself:

- `error_code_table_is_sorted_and_unique` — a sorted table makes a duplicate
  visible as an adjacent pair rather than requiring a set comparison.
- `error_code_table_has_no_ambiguous_discriminants` — a duplicate discriminant
  makes the on-chain error ambiguous, because two Rust variants decode from the
  same `u32` and a client cannot tell which condition occurred.

## Determinism and reproduction

Every fault decision is drawn from `ChaosRng`, a self-contained xorshift64*
generator seeded per campaign. This is a deliberate trade against `proptest`:
chaos campaigns are long and multi-fault, and a reproducible sequence is worth
more than a minimal one.

A failing campaign prints its seed in the summary. Re-run with the same seed to
get the identical fault schedule:

```bash
SOROSTREAM_CHAOS_SEED=0xDEADBEEF cargo test -p sorostream-stream chaos
```

The seed can also be overridden per test through `campaign_seed(default)`, which
reads `SOROSTREAM_CHAOS_SEED` (decimal or `0x`-prefixed) and falls back to a
fixed per-test constant so CI runs are reproducible with no configuration.

```text
chaos campaign seed=0x4 steps=40 injected=18 ok=11 handled=22 recovered=5 findings=0
  injected: token_transfer_rejects=4, clock_rewind=3, stream_record_loss=2, sequence_skew=3
```

## A Soroban trap worth knowing about

The first version of `ChaosToken` offered a `FAIL_NEXT` mode: a counter in token
storage, decremented by `transfer`, that would fail exactly *N* calls.

**It never cleared.** The host rolls back every storage write made by a failing
invocation — including the decrement. The counter therefore stayed at `N`
forever and the fault became permanent, which reads exactly like a contract bug
and is not one.

The fix is structural rather than cosmetic: the countdown lives outside the
token, in `ChaosEnv::arm_token_failures`, and each `ChaosEnv::clear` re-arms the
mode from the host, where the arming write is its own successful invocation and
is therefore not rolled back. `token_fail_countdown_lives_in_harness_not_token`
pins the behaviour.

The general lesson for anything that mutates state and then fails: **the state
change does not happen.** Counters, latches and "already done" flags must be
advanced from outside the failing frame.

## Running

```bash
# All chaos tests
cargo test -p sorostream-stream chaos

# The framework's own self-tests
cargo test -p sorostream-stream chaos_rng
cargo test -p sorostream-stream error_code

# A single campaign with a specific seed
SOROSTREAM_CHAOS_SEED=0x1234 cargo test -p sorostream-stream chaos_campaign_seed_0x1
```

## Extending the framework

To add a fault kind:

1. Add the variant to `FaultKind` and to `FaultKind::ALL`.
2. Give it a `label()` and decide `needs_token()` / `is_survivable()`.
3. Add the injection arm in `ChaosEnv::inject`.
4. Add a campaign that exercises it and asserts the accounting it must preserve.

`labels_unique` and `token_side_faults_declare_that_they_need_a_token` will fail
if steps 1–2 are skipped, so the invariants hold as the fault list grows.
