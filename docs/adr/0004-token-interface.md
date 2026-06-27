# ADR-0004: SAC token interface via `token::Client`

## Status

Accepted

## Context

SoroStream locks and releases tokens on behalf of senders and recipients. The contract needs to interact with token contracts to:

1. Transfer tokens from sender to contract on `create_stream` / `top_up`.
2. Transfer tokens from contract to recipient on `withdraw`.
3. Transfer tokens from contract to sender on `cancel_stream` (refund).
4. Check sender balance for auto-renew eligibility.

Soroban supports two token standards:

- **Stellar Asset Contract (SAC)**: The built-in token contract that wraps classic Stellar assets (XLM, USDC, etc.) with a standard interface. The SDK provides `soroban_sdk::token::Client` for type-safe interaction.
- **Custom token contracts**: Any contract that exposes compatible `transfer`, `balance`, `approve` functions, potentially with additional methods.

## Decision

Use `soroban_sdk::token::Client` for all token interactions. The contract stores a single `token: Address` per stream and constructs a `token::Client` at each call site.

The contract does **not** use `token::StellarAssetClient` (which provides mint/burn/admin functions) for runtime operations — only in tests for minting test tokens. This keeps the contract compatible with any token that implements the standard SAC interface, not just Stellar-issued assets.

The `top_up` function validates that the provided `token` address matches the stream's stored `token` address (`TokenMismatch` error), preventing accidental deposits of the wrong token type.

## Alternatives considered

### Store a `token::Client` in the Stream struct

Rejected because `token::Client` contains a reference to `Env` and cannot be serialized into contract storage. The `Address` is the minimal storable reference; the client is reconstructed on each invocation.

### Accept arbitrary token interfaces via a trait

Would allow the contract to work with non-SAC tokens that have different function signatures. Rejected because:

- Soroban's cross-contract calls are address-based, not trait-based. The SDK's `token::Client` already generates the correct cross-contract call for any contract that implements the SAC interface.
- Custom token interfaces would require per-token adapter logic, adding complexity without a clear use case.
- All major Stellar ecosystem tokens (USDC, XLM, etc.) use the SAC interface.

### Support multiple tokens per stream

Would allow a stream to pay out in one token while being funded in another (e.g., fund with USDC, pay out in EURC). Rejected because:

- This would require an on-chain price oracle or DEX integration, massively increasing contract complexity.
- The current design cleanly separates payment streaming from token exchange.

## Consequences

### Positive

- Any Soroban token implementing the SAC `transfer`/`balance` interface works out of the box — no contract changes needed for new tokens.
- `token::Client` provides compile-time type checking for function signatures and argument types.
- Single-token-per-stream simplifies accounting: deposit, flow rate, and withdrawals are all in the same denomination.

### Negative

- The contract cannot interact with tokens that deviate from the SAC interface (e.g., tokens requiring `approve` before `transfer`). This is a theoretical limitation — no known Soroban tokens require this pattern.
- Constructing `token::Client` on every call has negligible but non-zero CPU cost compared to caching. Soroban's single-invocation model makes caching impossible across transactions anyway.

### Neutral

- The `token` field in `Stream` is an `Address` (not a token symbol or asset code). Callers must know the contract address of the token they want to stream, which is standard for Soroban interactions.
