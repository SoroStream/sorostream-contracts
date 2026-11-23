# Security Policy

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Please report them privately via [GitHub Security Advisories](https://github.com/your-org/sorostream-contracts/security/advisories/new). We will acknowledge your report within 48 hours and aim to release a patch within 14 days for confirmed critical issues.

Include in your report:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- Any suggested mitigations (optional)

---

## Trust Assumptions

The following components are **explicitly trusted** by the SoroStream contracts:

| Component | Trust level | Rationale |
|-----------|-------------|-----------|
| **Token contract** (`token::Client`) | Fully trusted | The token address is supplied by the stream creator. SoroStream makes no attempt to validate the token contract's correctness, fee model, or rebase behaviour. Callers are responsible for using well-audited SEP-41 tokens. |
| **Ledger timestamp** (`env.ledger().timestamp()`) | Trusted | All time accounting relies on the Stellar network's ledger-close timestamp. A validator coalition controlling ledger close times could manipulate stream vesting. |
| **Sender / recipient / cosigner addresses** | Trusted as supplied | SoroStream does not perform identity verification. It only enforces `require_auth()`. |
| **Soroban runtime / host functions** | Fully trusted | The contract has no defence against a compromised Soroban host. |

---

## Known Limitations

The following limitations are **by design** or **explicitly out of scope** for the current version:

### No MEV / front-running resistance
Stream creation, claim, renewal, and proposal approval are all single-transaction operations without commit-reveal or ordering protection. A validator or block builder with privileged ordering could:
- Front-run a `claim` with a `cancel` to reduce recipient payout.
- Race a `discard_proposal` against `approve_proposal`.

No mitigation is planned in v1. Consider time-locks or commit-reveal schemes for high-value streams.

### No oracle / price feeds
`rate_per_second` is denominated in raw token units. There is no integration with price oracles. USD-denominated streaming rates require an off-chain recalculation and a new stream.

### No partial-deposit streams
`deposit` must be exactly divisible by `duration`. Streams where the deposit does not divide cleanly will be rejected. This avoids off-by-one rounding exploits but limits flexibility.

### Pull-based auto-renewal only
Soroban has no on-chain scheduler. Auto-renewal (`renew()`) is pull-based and must be triggered by a transaction. A stream will not renew itself if no one calls `renew()`.

### No reentrancy guard
Soroban's host executes contract calls atomically and does not support callbacks, so classic EVM-style reentrancy is not applicable. Cross-contract reentrancy via token callbacks is theoretically possible with a malicious token; see the token trust assumption above.

### Integer overflow on very long streams
`rate_per_second * elapsed_seconds` is computed as `i128`. Streams with extremely high deposits and very long durations could theoretically overflow, but `i128` supports values up to ~1.7 × 10³⁸, which is well beyond any realistic token supply.

### Reputation score is append-only
`get_reputation` scores are stored in instance storage and increment monotonically on each `renew()`. There is no decay, dispute mechanism, or slash. Treat the score as a raw activity counter, not a security guarantee.

### 2-of-2 multisig proposals use ledger sequence for expiry
Proposal expiry is measured in ledger sequence numbers, not wall-clock time. Ledger close time varies (~5–6 s on average). An `approval_window_ledgers` of 100 corresponds to roughly 8–10 minutes but is not an exact wall-clock guarantee.

---

## Out-of-Scope Attack Vectors

The following are explicitly **out of scope** for the SoroStream bug bounty and security review. Auditors should not spend time on them:

1. **Malicious token contracts** — The token is trusted (see above). A token that steals funds on `transfer` is the token's bug, not SoroStream's.
2. **Social engineering / private key compromise** — Loss of sender or recipient keys is out of scope.
3. **Stellar network-level consensus attacks** — Validator set compromise, ledger rollback, or network halt.
4. **Gas / resource exhaustion on the Stellar network** — Stellar's fee market is outside contract scope.
5. **UI / front-end vulnerabilities** — This repo only covers the on-chain contract.
6. **Reputation gaming via self-streaming** — A sender can stream to themselves to inflate reputation. This is a known design trade-off.
7. **Griefing by never calling `renew()`** — If a sender never triggers renewal, the stream simply does not continue. Recipients who rely on renewal should monitor expiry off-chain.
8. **`discard_proposal` race between sender retraction and cosigner approval** — Both transactions are valid operations; the network's natural ordering resolves the race.

---

## Supported Versions

| Version | Supported |
|---------|-----------|
| `main`  | ✅ Yes     |
| Others  | ❌ No      |

Only the latest commit on `main` receives security patches.
