# sorostream-contracts

![Rust](https://img.shields.io/badge/Rust-1.84+-orange?logo=rust)
![Soroban SDK](https://img.shields.io/badge/soroban--sdk-22.0.0-blue)
![License](https://img.shields.io/badge/license-MIT-green)
![CI](https://github.com/SoroStream/sorostream-contracts/actions/workflows/test.yml/badge.svg)

Soroban smart contracts for **SoroStream** — a real-time payment streaming protocol on Stellar. Stream USDC by the second for salaries, subscriptions, vesting schedules, and grant disbursements.

## How It Works

1. **Sender** calls `create_stream()` locking USDC for a recipient over a defined duration.
2. Contract computes `flow_rate = amount / duration_seconds`.
3. **Recipient** calls `withdraw()` at any time to claim `flow_rate × elapsed_seconds`.
4. **Sender** can `cancel_stream()` — recipient gets earned amount, sender gets remainder.
5. **Sender** can `top_up()` to add more USDC, automatically extending the end time.
6. Streams can `auto_renew` — restarting automatically on completion.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust 1.84+ |
| Smart Contract SDK | soroban-sdk 22.0.0 |
| CLI | stellar-cli |
| CI | GitHub Actions |

## Local Setup

```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Add WASM target
rustup target add wasm32-unknown-unknown

# 3. Install Stellar CLI
cargo install --locked stellar-cli --features opt

# 4. Run tests
cargo test

# 5. Lint
cargo clippy -- -D warnings

# 6. Build WASM
stellar contract build
```

## Contract Function Reference

| Function | Description |
|----------|-------------|
| `create_stream(sender, recipient, token, amount, duration_seconds, auto_renew)` | Creates a new stream, returns `stream_id` |
| `withdraw(stream_id, recipient)` | Recipient claims all earned tokens |
| `cancel_stream(stream_id, sender)` | Cancels stream, splits balance |
| `top_up(stream_id, sender, amount)` | Adds tokens, extends duration |
| `get_stream(stream_id)` | Returns full `Stream` struct |
| `get_all_stream_ids(start, limit)` | Returns a paginated list of all stream IDs ever created |
| `get_claimable(stream_id)` | Returns currently claimable amount |
| `get_streams_by_sender(sender)` | Returns all streams for a sender |
| `get_streams_by_recipient(recipient)` | Returns all streams for a recipient |

## Testnet Deployment

| Contract | Address |
|----------|---------|
| StreamContract | See [deployments/testnet.json](./deployments/testnet.json) |

## Contributing via Drips Wave

This project participates in the **Stellar Wave Program** on [Drips Wave](https://drips.network/wave). Contributors earn rewards for resolving issues during weekly Wave sprints — funded by the Stellar Development Foundation, free for contributors to participate.

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the full workflow.

> **Note:** Do not start coding until assigned to an issue by a maintainer.
