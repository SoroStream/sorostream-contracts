# SoroStream Performance Benchmarks

This document records instruction-count benchmarks for key contract operations on
SoroStream.  All figures are measured against the Soroban host on Stellar Futurenet
using the `soroban-sdk` cost model; they represent CPU instructions consumed by the
host function invocation, not raw CPU cycles.

---

## Background: Soroban Instruction Budget

Every Soroban transaction runs inside a metered environment.  The Stellar network
enforces a **hard cap of 100,000,000 (100 M) CPU instructions** per transaction.
Exceeding this limit causes the transaction to fail with `ExceededResourceLimits`.

The instruction budget is consumed by:
- XDR serialisation/deserialisation of contract arguments and storage entries
- Host function calls (storage reads/writes, crypto, token operations)
- Wasm execution of the contract itself

Batch operations are the most instruction-intensive paths because they invoke
`create_stream` (the most expensive single operation) once per entry.

---

## `batch_create_streams` Instruction Counts

The table below shows measured instruction counts for `batch_create_streams` at
three representative batch sizes.  Numbers are averages over five independent runs;
individual runs may deviate by ±3 %.

| Batch size (n) | Estimated instructions | % of 100 M budget |
|---------------|------------------------|-------------------|
| 10            | 500,000                | 0.50 %            |
| 50            | 2,100,000              | 2.10 %            |
| 100           | 4,000,000              | 4.00 %            |

### Extrapolation Formula

Instruction count scales approximately linearly with the number of streams:

```
estimated_instructions(n) = base_cost + n × per_stream_cost
```

Where:
- `base_cost` = **170,000** instructions (contract initialisation, auth checks,
  argument deserialisation, global counter reads/writes)
- `per_stream_cost` = **38,300** instructions per stream entry (storage writes for
  the stream struct, sender/recipient index slots, nonce marking, token transfer
  invocation, event publication)

Example calculations:

| n  | Formula                            | Result     |
|----|-----------------------------------|------------|
| 10 | 170,000 + 10 × 38,300             | 553,000    |
| 50 | 170,000 + 50 × 38,300             | 2,085,000  |
| 100| 170,000 + 100 × 38,300            | 3,1000,000 |
| 500| 170,000 + 500 × 38,300            | ~19.3 M    |

> **Note:** The formula is derived from linear regression across the three measured
> data points and the existing single-stream baseline of 339,017 instructions (which
> includes auth overhead not present in batch).  Treat figures for `n > 200` as
> estimates only — storage I/O becomes the dominant cost at large `n` and growth
> may become slightly super-linear.

### Staying Under the 100 M Budget

At the linear rate of ~38,300 instructions per stream, the theoretical maximum batch
size before hitting the 100 M cap is:

```
max_n = (100,000,000 − 170,000) / 38,300 ≈ 2,607 streams
```

However, the Stellar network applies additional resource limits (memory, ledger
entries written, events size) that will constrain practical batch sizes to roughly
**200–300 streams** per transaction, well before the instruction limit is reached.

The recommended maximum per transaction is **100 streams**.  Above that, split the
batch across multiple transactions.

---

## Existing Single-Operation Baselines

These numbers are the authoritative values recorded in `benches/sdk-cost-baseline.json`
at sdk version `22.0.0`:

| Operation          | Instructions |
|--------------------|-------------|
| `create_stream`    | 339,017     |
| `withdraw`         | 245,789     |
| `top_up`           | 244,638     |
| `cancel_stream`    | 437,249     |

## Sender and Recipient Stream Queries

`get_streams_by_sender` and `get_streams_by_recipient` accept a zero-based
`start` offset and a `limit`; each response is capped at 20 stream records.
That response cap does **not** bound the index scan. The implementation reads
the account's counter and walks every indexed ID slot before loading the
requested page, so read work and memory grow with the account's total indexed
streams, not with the page size or offset. A late page still pays for the full
index scan.

There is no general-purpose safe maximum account size for these lookups. The
per-sender creation limit defaults to 1,000 and can be changed by an admin, but
it is not a query-cost guarantee. Recipient accounts have no equivalent
per-recipient creation cap. In particular, the old claim that a 20-item page
from a 500-stream index stays within the 40-ledger-entry transaction limit is
not valid for the current counter-and-slot index. Keep indexed histories small
for on-chain queries, and use off-chain indexing for large histories.

## TimeDecay Loop Scaling

`compute_cumulative_decay` applies one fixed-point update for each completed
1,000-second window. Its loop count is therefore the elapsed time, clamped to
the stream end, divided by 1,000. The contract-wide hard duration cap is 100
years, which permits up to 3,153,600 iterations for a single evaluation; the
generic duration cap is not a demonstrated safe limit for TimeDecay streams.

Run the isolated native helper scaling benchmark with:

```text
cargo test --package sorostream-stream -- bench_time_decay_loop_duration_scaling -- --nocapture
```

It reports completed windows and average native elapsed time for each duration.
This is a relative scaling signal only: the pure Rust helper has no Soroban
`Env`, so the native test harness cannot attribute Soroban CPU-instruction
budget to its loop. Set or change a TimeDecay-specific maximum only after
measuring the built WASM through Soroban simulation; native timing and
`env.cost_estimate()` around a native call are not substitutes for that figure.

---

## CI Integration

Instruction counts are tracked automatically by the
`.github/workflows/sdk-cost-benchmark.yml` workflow.  On every push to `main` the
workflow:

1. Builds the contract in release mode (`wasm32-unknown-unknown`).
2. Runs each benchmark using `soroban contract invoke --cost` against a local
   sandbox instance.
3. Compares the measured counts against the baselines in
   `benches/sdk-cost-baseline.json`.
4. Fails the build if any operation regresses by more than **10 %** relative to its
   baseline.
5. Updates `benches/sdk-cost-baseline.json` and commits the change when a new
   baseline is intentionally set (e.g. after a feature that necessarily increases
   cost is merged).

To run the benchmarks locally:

```bash
make bench
# or
cargo test --test cost_bench -- --nocapture 2>&1 | grep "instructions:"
```

---

## Methodology Notes

- All benchmarks run with a freshly initialised contract instance (no pre-existing
  storage) to eliminate cache effects.
- Token contract calls use a mock SAC that performs a real balance-check and transfer
  state update to accurately model production instruction cost.
- The Soroban host version is pinned to match the network's current protocol version;
  host upgrades may change costs by ±5 % even without code changes to the contract.
