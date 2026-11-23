# Pre-authorization for pull-based entry points

A pull-based entry point can be submitted by a keeper or other third party, so the
funds owner may not be present to sign that transaction. The entry point must
still have explicit, bounded authorization to move funds. A caller's ability to
trigger an operation is never itself permission to spend someone else's tokens.

Before relying on a third party to trigger a pull, the owner must authorize the
specific contract as spender through the token's standard allowance mechanism.
The contract should then use `transfer_from` with itself as spender, the owner as
the source, and the intended destination and amount. The owner can revoke or
reduce the allowance through the token contract. The operation should fail
atomically when the allowance is absent, expired, or too small; it must not fall
back to caller-provided funds or broader authority. Keep the pull amount and
trigger conditions tied to contract state, validate the operation before moving
funds, and update state only as part of the same successful transaction.

## Worked example: `streaming::renew()`

`StreamingContract::renew(stream_id)` is permissionless to call after an
auto-renewing stream expires. It settles any unclaimed vested amount, then
pulls the next cycle's deposit from `stream.sender` using the streaming contract's
token allowance. The sender authorizes this recurring pull in advance; any keeper
may submit the renewal transaction, but cannot choose a different sender, token,
or deposit amount. If the allowance cannot cover the configured deposit, the
renewal fails and Soroban transaction rollback preserves the prior stream state.

The authorization bug was tracked in [#708](https://github.com/SoroStream/sorostream-contracts/issues/708).
Its third-party-call and allowance regression coverage is tracked in
[#821](https://github.com/SoroStream/sorostream-contracts/issues/821). Update this
documentation if the entry point's authorization model changes.

Apply the same review to any future keeper-triggered pull: identify the owner,
spender, exact amount, scope and lifetime of authorization, revocation path, and
failure behavior. Do not add a live owner `require_auth()` requirement to a
permissionless keeper trigger when the intended consent was already given through
an allowance.
