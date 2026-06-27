# Storage Layout and Key Encoding

This document is the authoritative reference for every storage key used by the SoroStream contract. It covers key encoding, durability assignments, size estimation, cleanup behavior, and upgrade safety.

Source: `contracts/stream/src/storage.rs`

---

## Storage key reference

### Instance storage (global, survives upgrades)

These keys are fixed-cardinality contract configuration. They live in instance storage, which is tied to the contract instance and preserved across WASM upgrades.

| Key literal | Type | Value type | Description |
|-------------|------|------------|-------------|
| `Symbol("admin")` | `Symbol` | `Address` | Contract administrator |
| `Symbol("paused")` | `Symbol` | `bool` | Emergency pause flag |
| `Symbol("next_id")` | `Symbol` | `u64` | Next stream ID counter (monotonically increasing) |
| `Symbol("fee_bps")` | `Symbol` | `u32` | Protocol fee in basis points (0–10000) |
| `Symbol("treasury")` | `Symbol` | `Address` | Fee recipient address |

### Persistent storage (per-stream and per-address)

These keys store stream records, per-address indexes, and idempotency guards. They must all share the same durability to avoid silent inconsistency (see ADR-0003).

| Key encoding | Key type | Value type | Description |
|-------------|----------|------------|-------------|
| `stream_id` | `u64` | `Stream` struct | Full stream record |
| `(Symbol("sc"), address)` | `(Symbol, Address)` | `u32` | Sender stream count |
| `(Symbol("s"), address, idx)` | `(Symbol, Address, u32)` | `u64` | Sender index slot: stream ID at position `idx` |
| `(Symbol("rc"), address)` | `(Symbol, Address)` | `u32` | Recipient stream count |
| `(Symbol("r"), address, idx)` | `(Symbol, Address, u32)` | `u64` | Recipient index slot: stream ID at position `idx` |
| `(Symbol("n"), address, nonce)` | `(Symbol, Address, u64)` | `bool` | Nonce guard (idempotency) |

### Temporary storage

**Not used.** Do not reintroduce for any data that clients rely on after the creating transaction completes. See the [storage durability decision](adr/0003-storage-layout.md) and [STORAGE.md](STORAGE.md) for the rationale.

---

## Per-stream vs global keys

```
┌─────────────────────────────────────────────────────────────┐
│                   INSTANCE STORAGE (global)                  │
│                                                              │
│   "admin"    → Address     (contract administrator)          │
│   "paused"   → bool        (emergency pause flag)            │
│   "next_id"  → u64         (stream ID counter)               │
│   "fee_bps"  → u32         (protocol fee)                    │
│   "treasury" → Address     (fee recipient)                   │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                 PERSISTENT STORAGE (per-stream)              │
│                                                              │
│   0 → Stream { id:0, sender:A, recipient:B, ... }           │
│   1 → Stream { id:1, sender:A, recipient:C, ... }           │
│   2 → Stream { id:2, sender:D, recipient:B, ... }           │
│   ...                                                        │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│               PERSISTENT STORAGE (per-address)               │
│                                                              │
│   Sender A indexes:                                          │
│   ("sc", A)    → 2          (count of streams by A)          │
│   ("s", A, 0)  → 0          (1st stream ID)                  │
│   ("s", A, 1)  → 1          (2nd stream ID)                  │
│                                                              │
│   Sender D indexes:                                          │
│   ("sc", D)    → 1                                           │
│   ("s", D, 0)  → 2                                           │
│                                                              │
│   Recipient B indexes:                                       │
│   ("rc", B)    → 2                                           │
│   ("r", B, 0)  → 0                                           │
│   ("r", B, 1)  → 2                                           │
│                                                              │
│   Recipient C indexes:                                       │
│   ("rc", C)    → 1                                           │
│   ("r", C, 0)  → 1                                           │
│                                                              │
│   Nonce guards:                                              │
│   ("n", A, 0)  → true                                        │
│   ("n", A, 1)  → true                                        │
│   ("n", D, 0)  → true                                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Storage size estimation

### Per-stream cost

Each stream creation writes the following persistent entries:

| Entry | Key size (bytes) | Value size (bytes) | Count |
|-------|----------------:|-------------------:|------:|
| Stream record | 8 (u64) | ~180 (3 Addresses + 2 i128 + 4 u64 + enum + bool) | 1 |
| Sender count | ~40 (Symbol + Address) | 4 (u32) | 1 |
| Sender slot | ~44 (Symbol + Address + u32) | 8 (u64) | 1 |
| Recipient count | ~40 (Symbol + Address) | 4 (u32) | 1 |
| Recipient slot | ~44 (Symbol + Address + u32) | 8 (u64) | 1 |
| Nonce guard | ~44 (Symbol + Address + u64) | 1 (bool) | 1 |

**Estimated total per stream: ~425 bytes** across 6 persistent entries.

### Formula

For a contract with **S** unique senders, **R** unique recipients, and **N** total streams:

```
Instance storage:  ~5 entries, ~150 bytes total (fixed)
Persistent entries: N (streams) + 2S (sender counts+slots) + 2R (recipient counts+slots) + N (nonces)
                  = 2N + 2S + 2R entries

If every stream has a unique sender and recipient (worst case):
  Total persistent entries = 2N + 2N + 2N = 6N
  Total persistent bytes   ≈ 425 × N
```

### Ledger entry limits

Soroban Protocol 22 limits per transaction:

| Resource | Limit |
|----------|------:|
| Read ledger entries | 40 |
| Write ledger entries | 25 |

Key operations and their entry counts:

| Operation | Read entries | Write entries |
|-----------|------------:|-------------:|
| `create_stream` | ~4 | 6 |
| `withdraw` | ~3 | 1–2 |
| `cancel_stream` | ~3 | 1 |
| `get_streams_by_sender(N)` | 1 + N + N | 0 |
| `batch_create_stream(N)` | ~3 | 1 + 5N |

The safe maximum for `get_streams_by_sender`/`get_streams_by_recipient` pagination is **~18 streams** per page (1 counter read + 18 slot reads + 18 stream reads = 37 entries).

The safe maximum for `batch_create_stream` is **4 streams** per call (1 + 5×4 = 21 write entries).

---

## Cleanup on stream completion

When a non-auto-renew stream reaches its end time and the recipient calls `withdraw`:

| What happens | Storage effect |
|-------------|----------------|
| Stream record is **deleted** | `remove_stream(env, stream_id)` removes the `u64 → Stream` persistent entry |
| Sender/recipient index slots are **NOT deleted** | The slot entries `("s", addr, idx)` and `("r", addr, idx)` remain. Reads filter stale entries at query time by checking if `load_stream` returns `Some`. |
| Nonce guard is **NOT deleted** | `("n", sender, nonce) → true` persists to prevent stream ID reuse. |
| ID counter is **NOT decremented** | `"next_id"` only increments. Deleted stream IDs leave gaps. |

When a stream is **cancelled** (full or partial):

| What happens | Storage effect |
|-------------|----------------|
| Stream status set to `Cancelled` | The stream record remains in storage with `status: Cancelled`. It is **not** deleted. |
| For partial cancel: new stream created | A new stream record, index slots, and events are written. |

**Auto-renew streams** are never deleted — they reset their time window and remain in storage.

---

## Upgrade safety — keys that must not be reused

After a WASM upgrade via `upgrade()`, all instance and persistent storage entries are preserved with their original keys. New contract versions **must not** reuse or reinterpret existing key patterns:

| Key pattern | Reason |
|-------------|--------|
| Bare `u64` keys | Reserved for `Stream` records. A new feature must not store non-Stream data under bare integer keys. |
| `Symbol("admin")`, `Symbol("paused")`, `Symbol("next_id")`, `Symbol("fee_bps")`, `Symbol("treasury")` | Reserved instance config keys. Changing their value type would corrupt contract state. |
| `("sc", addr)`, `("s", addr, idx)` | Sender index keys. Changing the value type or semantics would break `get_streams_by_sender`. |
| `("rc", addr)`, `("r", addr, idx)` | Recipient index keys. Same constraint. |
| `("n", addr, nonce)` | Nonce guards. Removing or resetting these would allow duplicate stream creation. |

When adding new storage keys in an upgrade:

1. Use a **unique Symbol prefix** that does not collide with `"s"`, `"r"`, `"sc"`, `"rc"`, `"n"`, or any instance key name.
2. Document the new key in this file before merging.
3. Consider whether existing persistent entries need migration — Soroban does not run migration scripts automatically.
