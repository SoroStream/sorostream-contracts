# SoroStream Contracts

A Soroban streaming payment contract for the Stellar network.

## Features

- **Streaming payments** — create time-based token streams with per-second vesting
- **Auto-renewal** — pull-based renewal preserving exact time-window accounting (issue #8 fix)
- **Reputation scoring** — per-sender score incremented on each natural completion
- **2-of-2 multisig proposals** — stream creation requiring co-signer approval within a configurable ledger window

## Project Structure

```
contracts/
  streaming/
    src/
      lib.rs       # contract entry points
      types.rs     # Stream, StreamProposal, DataKey
      storage.rs   # persistent/instance storage helpers
      test.rs      # full test suite
```

## Building

```sh
# Install WASM target once
rustup target add wasm32-unknown-unknown

make build   # compile to WASM
make test    # run all tests
make lint    # clippy
make fmt     # rustfmt
```

## Security

See [SECURITY.md](SECURITY.md) for trust assumptions, known limitations, and responsible disclosure instructions.
