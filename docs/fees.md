# SoroStream Fee Documentation

This document describes the SoroStream fee model end-to-end: how fees are configured, when they are deducted, where the tokens go, and how operators manage the treasury. Worked numerical examples are included throughout.

---

## Overview

SoroStream has two independent fee mechanisms:

| Fee | Token | When charged | Storage key |
|-----|-------|-------------|-------------|
| **Protocol fee** | Streaming token (e.g. USDC) | On every `withdraw` call | `"fee_bps"` (instance) |
| **Creation fee** | XLM (stroops) | At `create_stream` time | `"cf_xlm"` (instance) |

Both fees go directly to the **treasury address** configured in the stream contract (`"treasury"` key). The treasury contract then accumulates and distributes those tokens to operators and liquidity providers.

---

## 1. Protocol Fee (Basis Points on Withdrawals)

### 1.1 Configuration

The protocol fee is stored as `fee_bps` — an unsigned 32-bit integer representing a fraction of 10,000 (i.e., basis points, where 100 bps = 1%).

```
Maximum allowed: 10,000 bps (100%)
Default (uninitialised): 0 bps (no fee)
```

**Reading the current fee:**
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --network testnet \
  -- get_protocol_fee_info
# returns: (fee_bps: u32, treasury: Option<Address>)
```

Protocol fee changes are timelocked for seven days. `set_protocol_fee` and
`propose_fee_change` create a pending proposal; the current fee is unchanged
until anyone calls `execute_fee_change` after the unlock time. This delay
prevents an administrator from raising the fee immediately before a large
withdrawal.

### 1.2 Fee Deduction in `withdraw`

Every call to `withdraw(stream_id, recipient)` goes through this calculation:

```
claimable = flow_rate × elapsed_seconds   (see vesting_math::compute_claimable)

if fee_bps > 0 AND recipient is NOT fee-exempt:
    fee_amount      = claimable × fee_bps / 10_000    (integer division, rounds down)
    recipient_amount = claimable - fee_amount
else:
    fee_amount      = 0
    recipient_amount = claimable
```

Source: `contracts/stream/src/lib.rs`, `withdraw()`:
```rust
let fee_bps = get_protocol_fee(&env);
let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
    claimable
        .checked_mul(fee_bps as i128)
        .ok_or(StreamError::Overflow)?
        / 10_000
} else {
    0
};
let recipient_amount = claimable - fee_amount;
```

The fee is transferred **directly to the treasury address** in the same transaction — no intermediate accumulation inside the stream contract:

```rust
token_client.transfer(
    &env.current_contract_address(),
    t,          // treasury address
    &fee_amount,
);
events::fee_collected(&env, stream_id, fee_amount, t);
```

### 1.3 Worked Example — Mid-Stream Withdrawal

**Parameters:**
- Deposit: 1,000,000 USDC stroops (1 USDC, since USDC has 7 decimal places)
- Duration: 1,000 seconds
- `flow_rate` = 1,000,000 / 1,000 = **1,000 stroops/second**
- Protocol fee: **50 bps** (0.5%)
- Recipient is NOT fee-exempt

**Scenario:** Recipient calls `withdraw` after 300 seconds.

```
claimable        = 1,000 stroops/s × 300 s = 300,000 stroops
fee_amount       = 300,000 × 50 / 10,000  =   1,500 stroops
recipient_amount = 300,000 - 1,500         = 298,500 stroops
```

**Token flows in this transaction:**
```
Stream contract  →  recipient  : 298,500 stroops USDC
Stream contract  →  treasury   :   1,500 stroops USDC
```

**Event emitted:**
```
StreamWithdrawn(stream_id, recipient, 300_000, timestamp)
FeeCollected(stream_id, 1_500, treasury_address)
```

### 1.4 Worked Example — Withdrawal at End of Stream

**Same stream as above.** Recipient did not withdraw at 300 s and now calls `withdraw` at 1,000 s (stream has ended).

```
claimable        = 1,000 × 1,000 = 1,000,000 stroops
fee_amount       = 1,000,000 × 50 / 10,000 = 5,000 stroops
recipient_amount = 1,000,000 - 5,000       = 995,000 stroops
dust             = deposit - (flow_rate × duration)
                 = 1,000,000 - (1,000 × 1,000) = 0 stroops
```

```
Stream contract  →  recipient  : 995,000 stroops USDC
Stream contract  →  treasury   :   5,000 stroops USDC
Stream contract  →  sender     :       0 stroops  (no dust)
```

**Events:**
```
StreamWithdrawn(stream_id, recipient, 1_000_000, timestamp)
FeeCollected(stream_id, 5_000, treasury_address)
StreamCompleted(stream_id)
```

### 1.5 Worked Example — Dust (Deposit Not Divisible by Duration)

**Parameters:**
- Deposit: 1,000,003 stroops USDC
- Duration: 1,000 seconds
- `flow_rate` = 1,000,003 / 1,000 = **1,000 stroops/second** (integer floor)
- Dust = 1,000,003 − (1,000 × 1,000) = **3 stroops**

At end of stream, after fee is applied to the claimable portion:
```
claimable        = 1,000 × 1,000 = 1,000,000 stroops
fee_amount       = 1,000,000 × 50 / 10,000 = 5,000 stroops
recipient_amount = 995,000 stroops
dust             = 3 stroops  → returned to sender
```

```
Stream contract  →  recipient  : 995,000 stroops
Stream contract  →  treasury   :   5,000 stroops
Stream contract  →  sender     :       3 stroops
```

> **The 3-stroop dust is NOT subject to a protocol fee** — it is an accounting artefact from integer division and is always returned in full to the sender.

### 1.6 Fee Exemption

Specific addresses can be marked fee-exempt. When an exempt recipient withdraws, `fee_amount = 0` and `recipient_amount = claimable`.

```bash
# Grant exemption (admin only)
stellar contract invoke --id $CONTRACT_ID --source admin-key --network testnet \
  -- add_fee_exempt --addr $RECIPIENT_ADDRESS

# Remove exemption
stellar contract invoke --id $CONTRACT_ID --source admin-key --network testnet \
  -- remove_fee_exempt --addr $RECIPIENT_ADDRESS

# Check exemption
stellar contract invoke --id $CONTRACT_ID --network testnet \
  -- is_fee_exempt --addr $RECIPIENT_ADDRESS
```

### 1.7 Fee on `transfer_recipient`

When a recipient transfers their stream to a new address via `transfer_recipient`, any accrued claimable tokens are automatically swept to the old recipient first. That sweep applies the same fee logic:

```rust
let fee_amount = if fee_bps > 0 && !is_fee_exempt(&env, &stream.recipient) {
    claimable * fee_bps as i128 / 10_000
} else {
    0
};
```

A `StreamWithdrawn` + `FeeCollected` event pair is emitted for this implicit withdrawal.

---

## 2. Creation Fee (Flat XLM)

### 2.1 Configuration

The creation fee is a flat amount of XLM stroops charged once per `create_stream` call. It is stored under the `"cf_xlm"` instance storage key and requires a configured XLM SAC token address.

```
1 XLM = 10,000,000 stroops
Default (uninitialised): 0 (disabled)
```

**Set the creation fee (admin only):**
```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- set_creation_fee \
  --fee 5000000 \
  --xlm_token $XLM_SAC_ADDRESS
# sets a 0.5 XLM creation fee
```

**Read the current fee:**
```bash
stellar contract invoke --id $CONTRACT_ID --network testnet -- get_creation_fee
# returns: i128 (stroops)
```

### 2.1 Deposit creation tax

The admin may configure either a flat tax in the stream deposit token or a
basis-point tax on the gross deposit. The tax is transferred to the configured
treasury when `create_stream` is called, and the stream's stored `deposit` is
the post-tax amount.

```bash
stellar contract invoke --id $CONTRACT_ID --source admin-key --network testnet \
  -- set_creation_tax --flat_amount 100000 --fee_bps 0
# Or use 250 bps (2.5%) instead:
stellar contract invoke --id $CONTRACT_ID --source admin-key --network testnet \
  -- set_creation_tax --flat_amount 0 --fee_bps 250
```

Set both values to zero to disable the tax. The flat amount and percentage
cannot be enabled at the same time. A tax that consumes the full deposit is
rejected, so every created stream has a positive post-tax deposit.

### 2.2 Deduction in `create_stream`

Before locking the streaming token deposit, the contract checks `cf_xlm`:

```rust
let creation_fee = get_creation_fee_xlm(&env);
if creation_fee > 0 {
    let treasury = get_treasury(&env).ok_or(StreamError::NotInitialized)?;
    let xlm_token = get_xlm_token(&env).ok_or(StreamError::NotInitialized)?;
    token::Client::new(&env, &xlm_token).transfer(
        &sender,
        &treasury,
        &creation_fee,
    );
    events::creation_fee_collected(&env, creation_fee, &treasury);
}
```

If `cf_xlm > 0` but the treasury or XLM token is not configured, the call returns `NotInitialized`.

If the sender's XLM balance is insufficient, the token transfer panics with `InsufficientBalance` (error 7). The stream is **not** created.

### 2.3 Worked Example — Creation Fee

**Parameters:**
- Creation fee: 5,000,000 stroops (0.5 XLM)
- Stream deposit: 10,000,000 USDC stroops

**On `create_stream` call:**
```
sender   →  treasury  : 5,000,000 stroops XLM
sender   →  contract  : 10,000,000 stroops USDC (deposit locked)
```

**Events:**
```
CreationFeeCollected(5_000_000, treasury_address)
StreamCreated(stream_id, sender, recipient, 10_000_000, flow_rate, end_time)
```

---

## 3. Fee Change Governance (7-Day Timelock)

Protocol fee changes are guarded by a two-step timelock to give users advance notice.

### Step 1 — Propose

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- propose_fee_change \
  --admin $ADMIN_ADDRESS \
  --new_fee_bps 100
```

- Requires admin auth.
- Sets `pnd_fee = (100, now + 604_800)` in instance storage.
- Emits `FeeChangeProposed(new_fee=100, unlock_time=<now+7days>)`.

### Step 2 — Execute (after 7 days)

```bash
stellar contract invoke \
  --id $CONTRACT_ID \
  --network testnet \
  -- execute_fee_change
```

- Can be called by anyone after `unlock_time` has passed.
- Sets `fee_bps = 100`, clears `pnd_fee`.
- Emits `FeeChangeExecuted(new_fee=100)`.

### Timeline

```
Day 0   propose_fee_change(100)  → FeeChangeProposed
Day 0–7 old fee still active
Day 7+  execute_fee_change()     → FeeChangeExecuted, new fee = 100 bps
```

---

## 4. Treasury Contract

All fees are transferred directly to the **treasury contract address**. The treasury contract (`contracts/treasury`) accumulates balances and supports two distribution modes.

### 4.1 Balance Tracking

The treasury tracks per-token balances in persistent storage:

```
key = ("balance", token_address)
value = i128 (cumulative stroops received)
```

The stream contract optionally calls `treasury.deposit(token, amount)` (a best-effort cross-contract call) to update this ledger. The token transfer happens regardless; the deposit call is a bookkeeping step only.

### 4.2 Withdrawal by Admin

```bash
# Withdraw a specific amount
stellar contract invoke \
  --id $TREASURY_CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- withdraw_treasury \
  --token $USDC_ADDRESS \
  --amount 50000 \
  --destination $DESTINATION_ADDRESS

# Withdraw everything
stellar contract invoke \
  --id $TREASURY_CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- withdraw_all \
  --token $USDC_ADDRESS \
  --destination $DESTINATION_ADDRESS
```

### 4.3 Distribute (Treasury + LP Split)

The `distribute` function splits accumulated fees between a treasury wallet and an LP reward pool according to a configurable basis-point split.

**Configure the split:**
```bash
# 70% to treasury, 30% to LP pool
stellar contract invoke \
  --id $TREASURY_CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- set_treasury_split \
  --treasury_bps 7000

stellar contract invoke \
  --id $TREASURY_CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- set_lp_pool \
  --lp_pool $LP_POOL_ADDRESS
```

**Run the distribution:**
```bash
stellar contract invoke \
  --id $TREASURY_CONTRACT_ID \
  --source admin-key \
  --network testnet \
  -- distribute \
  --token $USDC_ADDRESS \
  --destination $TREASURY_WALLET
```

**Worked example — distribute 100,000 USDC stroops (70/30 split):**
```
total        = 100,000 stroops
treasury_bps = 7,000
treasury     = 100,000 × 7,000 / 10,000 = 70,000 stroops  → $TREASURY_WALLET
lp_amount    = 100,000 - 70,000          = 30,000 stroops  → $LP_POOL_ADDRESS
```

**Event emitted:**
```
FeeDistributed(token, 70_000, 30_000)
```

---

## 5. End-to-End Flow Summary

```
create_stream()
  └─ if cf_xlm > 0:
       sender → treasury (XLM)
       emit CreationFeeCollected

withdraw()
  ├─ compute claimable (vesting_math)
  ├─ fee_amount = claimable × fee_bps / 10_000
  ├─ contract → recipient   (claimable - fee_amount)  [USDC]
  ├─ contract → treasury    (fee_amount)               [USDC]
  ├─ emit StreamWithdrawn(claimable)
  └─ emit FeeCollected(fee_amount, treasury)

treasury.distribute()
  ├─ treasury_amount = total × treasury_bps / 10_000
  ├─ lp_amount       = total - treasury_amount
  ├─ treasury → $TREASURY_WALLET  (treasury_amount)
  ├─ treasury → $LP_POOL          (lp_amount)
  └─ emit FeeDistributed
```

---

## 6. Reference — Storage Keys

| Key | Contract | Storage tier | Type | Description |
|-----|---------|-------------|------|-------------|
| `"fee_bps"` | stream | Instance | `u32` | Protocol fee in basis points |
| `"treasury"` | stream | Instance | `Address` | Treasury contract address |
| `"cf_xlm"` | stream | Instance | `i128` | Flat XLM creation fee (stroops) |
| `"pnd_fee"` | stream | Instance | `(u32, u64)` | Pending timelock proposal |
| `("balance", token)` | treasury | Persistent | `i128` | Accumulated fee balance per token |
| `"t_split"` | treasury | Instance | `u32` | Treasury split in bps |
| `"lp_pool"` | treasury | Instance | `Address` | LP reward pool address |

> Closes [#262](https://github.com/SoroStream/sorostream-contracts/issues/262).
