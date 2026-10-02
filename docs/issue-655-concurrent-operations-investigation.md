# Issue #655: Concurrent Stream Operations Investigation

Status: blocked pending a shared-ledger transaction test harness.

The current contract tests use a Soroban `Env` fixture and invoke client methods sequentially. Creating one `Env` per worker would run independent ledgers, so it could not expose conflicting writes or corruption in shared stream state. A fixed random seed makes generated operations repeatable, but it does not make thread scheduling or transaction interleavings deterministic.

To test the requested race behavior, the harness needs to submit concurrent transactions against shared ledger state and expose a deterministic way to reproduce conflicting schedules. The test must also define which outcomes are valid when operations target the same stream (for example, a withdrawal racing with cancellation). Without that host-level capability and operation policy, a multithreaded test would either test unrelated isolated environments or encode unspecified behavior.

Recommended next step: confirm whether a supported Soroban host/ledger runner can execute parallel transactions against one ledger with a reproducible schedule. If not, narrow the issue to seeded randomized sequential lifecycle testing, which can cover state invariants but does not claim to test thread races. This draft does not claim that concurrent shared-state execution is covered.