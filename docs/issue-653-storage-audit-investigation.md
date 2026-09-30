# Issue #653: Storage I/O Audit Investigation

Status: blocked pending an instrumentation boundary for test execution.

The current stream implementation reads and writes through direct `env.storage()` calls across contract entry points and storage helpers. The existing storage-layout snapshot tests preserve the final ledger state after selected tests; they do not record reads, identify individual writes, or detect a mutation that is later reverted before the snapshot.

Capturing every storage operation requires either a supported Soroban test-host recorder or routing all contract storage access through an instrumentable adapter. A test-only adapter must still cover reads and writes made by every contract path without changing production semantics. The audit format also needs to identify the test and operation, and to define how sensitive stored values are represented in CI artifacts.

Recommended next step: confirm whether maintainers want a broad storage-access refactor or whether an upstream host-level recorder is available/acceptable. Then define the audit event schema and mutation policy (for example, expected writes versus unexpected key changes) before making the recorder a CI gate. This note does not claim that the current snapshots provide a complete I/O audit trail.