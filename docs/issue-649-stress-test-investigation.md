# Issue #649: 10,000-Stream Stress Test Investigation

Status: blocked pending a benchmark scope and CI performance budget.

The stream contract tests run against Soroban's in-process test environment. A measured wall-clock duration there is the runtime of a particular test host and CI runner; it is not the latency of an on-chain invocation. A hard 100 ms assertion would therefore vary with runner load and would not establish a contract-level latency guarantee.

A meaningful benchmark also needs to define whether the 10,000 streams are created in one environment and transaction sequence, how sender limits and rate limits are configured, and whether the reported withdraw, cancel, and top-up values are individual samples or aggregate timings. Without these conditions, a passing result is not comparable across runs and a failing result may only reflect setup or runner contention.

Recommended next step: agree on a dedicated, stable CI runner and test-host resource budget, then add a stress workload that reports setup time separately from per-operation samples (including a distribution, not only a single measurement). Establish a measured baseline before enforcing a threshold. This draft does not claim the 10,000-stream workload or 100 ms limit has been validated.