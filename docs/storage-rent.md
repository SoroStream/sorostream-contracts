# Storage Rent in SoroStream

Soroban charges **rent** for keeping data in contract storage. This document explains the rent
model, how it applies to SoroStream, and how operators can budget for and reduce rent costs over
time.

---

## How Soroban Rent Works

Soroban storage is split into three tiers, each with different persistence and rent semantics:

| Tier | Key characteristics | Rent behaviour |
|------|-------------------|----------------|
| **Instance** | Tied to the contract instance; lives as long as the contract exists | Kept alive automatically when any contract function is invoked |
| **Persistent** | Long-lived entries (streams, indices, nonces) | Expires after a `ledger_ttl`; must be extended with `extend_ttl` |
| **Temporary** | Short-lived data that auto-expires | No renewal; expires after a fixed number of ledgers |

SoroStream uses **instance** storage for global configuration (admin, version, fee, pause flag,
audit log) and **persistent** storage for each stream entry plus all lookup indices.

When a persistent entry's TTL reaches zero it is evicted. The data is gone unless it was
archived elsewhere. Operators must call `extend_ttl` on entries they want to keep alive.

---

## Rent Cost Formula

The network charges rent proportional to the **number of bytes** stored and the **number of
ledgers** an entry is kept alive:

```
rent_per_entry_per_ledger ≈ entry_size_bytes × rent_fee_per_byte_per_ledger
```

For a SoroStream deployment with *N* active streams, the approximate ongoing storage cost is:

```
monthly_rent ≈ N × entry_size_bytes × rent_fee_per_byte_per_ledger × ledgers_per_month
```

Where, using Stellar Mainnet reference values (subject to change via governance):

| Parameter | Approximate value |
|-----------|------------------|
| `entry_size_bytes` (one Stream entry) | ~500 bytes |
| `rent_fee_per_byte_per_ledger` | see [Stellar fee schedule](https://developers.stellar.org/docs/learn/fundamentals/fees-resource-limits-metering) |
| `ledgers_per_month` | ~432,000 (5 s/ledger × 2,592,000 s/month) |
| Index entries per stream | 3 (global + sender + recipient slot) |

A single active stream thus occupies roughly **4 persistent entries × ~500 bytes = ~2 KB** of
billed storage.

> **Note:** The exact `rent_fee_per_byte_per_ledger` is a network parameter voted on by validators.
> Always check the current schedule before budgeting.

---

## When to Expect Rent Bumps

Rent bumps are needed when a persistent entry's TTL is about to expire. Plan for bump
transactions in the following scenarios:

1. **At stream creation** — the entry is created with a default TTL. If the stream duration
   exceeds the default TTL, bump immediately.
2. **At regular cadence** — for long-running streams (vesting schedules, multi-year grants),
   schedule periodic `extend_ttl` calls before expiry. A safe heuristic is to bump when the
   remaining TTL drops below 30 days (~259,200 ledgers).
3. **After contract upgrades** — `upgrade()` may reset instance TTL; call `extend_ttl` on the
   instance after every upgrade.
4. **Before `withdraw()` on old streams** — if a stream entry has been idle for a long time,
   verify it is still live before the recipient tries to withdraw.

---

## Budgeting for Rent

A practical budget formula for a SoroStream operator:

```
annual_rent_xlm ≈
    N_streams
    × 4_entries_per_stream
    × 500_bytes_per_entry
    × rent_fee_per_byte_per_ledger
    × 6_307_200_ledgers_per_year
```

For example, with 1,000 concurrent streams at a hypothetical rent rate of
`0.000_000_01 XLM / byte / ledger`:

```
1_000 × 4 × 500 × 0.00000001 × 6_307_200 ≈ 1_261 XLM / year
```

This is a rough upper bound. Most streams settle within weeks, so the effective average active
count is typically much lower than the total count.

---

## Reducing Rent Over Time

Storage cleanup directly reduces ongoing rent costs:

- **`archive_stream(stream_id, caller)`** — After a stream is fully settled
  (`total_withdrawn == deposit`), call `archive_stream` to remove the stream entry and its three
  index entries from persistent storage. This immediately stops rent accruing for those ~2 KB.
- **Cancelled and completed streams** — The contract already removes storage entries on
  `cancel_stream()` and on `withdraw()` when a non-auto-renew stream reaches its end time.
  Senders should prompt recipients to withdraw promptly so cleanup happens automatically.
- **Nonce entries** — Each stream creation writes a nonce entry (`~50 bytes`). These are not
  cleaned up automatically. For very high-volume deployments consider tracking and removing
  expired nonces manually.

By combining regular `archive_stream` calls with a monitoring script that bumps TTLs before
expiry, operators can maintain predictable rent costs at any scale.

---

## Further Reading

- [Soroban Storage and TTL](https://developers.stellar.org/docs/learn/encyclopedia/storage/state-archival)
- [SoroStream Storage Layout](./storage-layout.md)
- [SoroStream STORAGE guide](./STORAGE.md)
