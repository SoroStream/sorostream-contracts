# Soroban Storage Model in SoroStream

This document explains how SoroStream uses Soroban's storage types, the trade-offs between them, and guidelines for contributors. Read this before adding or changing anything in `contracts/stream/src/storage.rs`.

## Soroban storage types (SDK 22)

Soroban exposes three durability levels through `env.storage()`:

| API | Durability | Typical use |
|-----|------------|-------------|
| `env.storage().instance()` | **Instance** — tied to the contract instance; survives WASM upgrades | Small, contract-wide configuration (admin, pause flag, counters) |
| `env.storage().persistent()` | **Persistent** — long-lived ledger entries keyed independently | User data that must outlive any single transaction (streams, indexes, idempotency keys) |
| `env.storage().temporary()` | **Temporary** — short-lived entries with a TTL | Per-transaction scratch data, caches that may be rebuilt, or data you explicitly extend on every use |

All three incur **resource fees** (CPU, memory, and storage rent on network). The choice is not only about cost — it determines whether data can **silently disappear** while other parts of the contract still look healthy.

### Instance storage

- **Pros:** Cheapest per entry for small values; naturally preserved across contract upgrades (`upgrade()` in `lib.rs` relies on this).
- **Cons:** Limited key space shared with all instance-scoped config; not suitable for unbounded per-user data (every sender/recipient would need their own keys at scale).
- **TTL:** Instance entries do not use the same eviction model as temporary storage, but they still participate in state archival economics on network.

### Persistent storage

- **Pros:** Correct default for protocol state that must remain queryable for the life of a stream (or longer). Each key is independent, so one entry expiring does not wipe unrelated data.
- **Cons:** Higher write/read cost than instance or temporary; unbounded keys (one slot per stream per address) grow rent linearly with usage. Keys must be designed carefully (see counter+slot pattern in `storage.rs`).
- **TTL:** Persistent entries have a **time-to-live** on network and must be **extended** (via `extend_ttl` or implicit extension on access, depending on protocol version) or they can eventually be archived. SoroStream treats stream records and indexes as permanent protocol state — any new persistent key must include a plan for TTL extension if the protocol requires it.

### Temporary storage

- **Pros:** Lowest cost for data only needed briefly; good for computation scratch space within a single invocation chain.
- **Cons:** Entries are **evicted when their TTL lapses** unless explicitly extended. There is **no error** when a key is missing — reads return `None`/empty, which looks like "no data" rather than "storage expired".
- **When not to use:** Any data that clients rely on after the creating transaction completes (indexes, balances, authorization flags, audit trails).

## How SoroStream maps data today

Implementation lives in `contracts/stream/src/storage.rs`.

### Instance storage (`instance()`)

| Key | Purpose |
|-----|---------|
| `admin` | Contract administrator |
| `paused` | Emergency pause flag |
| `next_id` | Monotonic stream ID counter |
| `fee_bps` | Protocol fee in basis points |
| `treasury` | Fee recipient address |

These are small, fixed-cardinality values read on many code paths. Keeping them in instance storage avoids mixing config with unbounded user state.

### Persistent storage (`persistent()`)

| Key pattern | Purpose |
|-------------|---------|
| `stream_id` (u64) | Full `Stream` struct |
| `("sc", sender)` | Sender index length counter |
| `("s", sender, idx)` | Nth stream ID for a sender |
| `("rc", recipient)` | Recipient index length counter |
| `("r", recipient, idx)` | Nth stream ID for a recipient |
| `("n", sender, nonce)` | Idempotency / duplicate-create guard |

Streams and their lookup indexes **must** use the same durability. If streams are persistent but indexes are temporary, `get_streams_by_sender` / `get_streams_by_recipient` can return empty vectors while `get_stream(id)` still succeeds — a silent, user-visible bug ([issue #1](https://github.com/SoroStream/sorostream-contracts/issues/1)).

The counter+slot layout replaces an earlier unbounded `Vec` in temporary storage (fixed in [issue #6](https://github.com/SoroStream/sorostream-contracts/issues/6)): each append is O(1) and avoids re-serializing an ever-growing vector.

### Temporary storage (`temporary()`)

Stream context metadata uses temporary storage under `("md", stream_id)`. Each
update extends its TTL to approximately 24 hours (17,280 ledgers), so callers
can refresh short-lived payroll, invoice, or IPFS context without expanding the
persistent `Stream` record. The metadata blob is capped at 256 bytes.

## The mistake we are documenting (issue #1)

The initial implementation stored:

- `Stream` records in **persistent** storage
- Sender/recipient index vectors in **temporary** storage

Temporary index entries expire when their TTL lapses. After expiration:

- `get_stream(stream_id)` still returns the stream
- `get_streams_by_sender(sender)` returns `[]`
- UIs and integrators believe the user has no streams

There is no revert, event, or error — only wrong query results. That is why index data was moved to **persistent** storage with counter+slot keys.

## Decision guide for contributors

When adding a new piece of contract state, ask:

1. **Must this survive beyond the current transaction?**  
   If yes → **not** temporary.

2. **Will off-chain clients or other contract functions read this later?**  
   If yes → persistent (or instance if it is global config).

3. **Is the data bounded (fixed number of keys)?**  
   Global config → instance. Per-user unbounded → persistent with an scalable key scheme.

4. **What happens if the key is missing?**  
   If missing data would cause **silent wrong behavior** (empty list, zero balance) rather than a clean `StreamError`, prefer persistent + explicit errors over temporary.

5. **Are you duplicating an index?**  
   If the canonical record lives in persistent storage, every derived index or cache must either use the same durability or be explicitly documented as best-effort with TTL extension on every write and read.

6. **Does `upgrade()` need to preserve this?**  
   Instance and persistent data survive WASM upgrades; document new keys in this file when merging.

## Related code and issues

- `contracts/stream/src/storage.rs` — all storage helpers
- `contracts/stream/src/lib.rs` — `upgrade()` notes that streams, indices, and counters are preserved
- [#1](https://github.com/SoroStream/sorostream-contracts/issues/1) — indexes must not live in temporary storage
- [#6](https://github.com/SoroStream/sorostream-contracts/issues/6) — counter+slot persistent indexes

## Further reading

- [Soroban storage documentation](https://developers.stellar.org/docs/build/smart-contracts/storage)
- [State archival and TTL](https://developers.stellar.org/docs/learn/fundamentals/contract-state-archival)
