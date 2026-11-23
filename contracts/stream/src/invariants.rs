//! Debug-mode accounting invariant assertions (issue #521).
//!
//! These checks run at the end of the contract's state-changing entry points in
//! **debug builds only**. The workspace release profile sets
//! `debug-assertions = false`, so in production every `debug_assert!` here is
//! compiled out — but `cargo test` and any audit/debug build exercise them. A
//! violation therefore fails loudly, naming the stream, instead of silently
//! corrupting protocol accounting.
//!
//! Two guarantees are checked:
//!
//! 1. **Per-stream sanity** ([`assert_stream_invariants`]) — a persisted stream
//!    record never carries a negative deposit, a negative cumulative withdrawal
//!    figure, a negative flow rate, or an end time before its start time.
//! 2. **Contract-wide solvency** ([`assert_token_conservation`]) — for every
//!    token, the contract's balance is at least the escrow still owed to the
//!    recipients of live streams. This is the ledger-wide form of "the sum of
//!    all active stream deposits must be covered by the contract's balance".
//!
//! # Why solvency is `>=`, not equality
//!
//! The contract legitimately holds balances on top of stream escrow:
//! accumulated protocol fees, unclaimed holdbacks, sender stakes and creation
//! fees. Asserting exact equality would therefore fire on entirely valid
//! states; `balance >= owed` is the property that protects recipients.
//!
//! # Why `total_withdrawn <= deposit` is not asserted per stream
//!
//! `update_stream_rate` intentionally *rebases*
//! `deposit` down to the remaining balance while `total_withdrawn` stays a
//! lifetime cumulative figure, so `total_withdrawn <= deposit` does not hold for
//! a stream whose rate was ever updated. The guarantee that check stands for —
//! "the contract can always cover what it owes" — is enforced by
//! [`assert_token_conservation`] instead.

use soroban_sdk::{token, Address, Env, Vec};

use crate::storage::{get_global_stream_at, get_global_stream_count, load_stream};
use crate::types::{Stream, StreamStatus};

/// Returns `true` while a stream still holds escrow the contract must back.
///
/// Settled statuses (`Completed`, `Cancelled`, `Expired`) are excluded: their
/// escrow has already been paid out or refunded, so they no longer contribute
/// to the amount the contract has to hold.
fn holds_escrow(status: &StreamStatus) -> bool {
    matches!(
        status,
        StreamStatus::Active
            | StreamStatus::Paused
            | StreamStatus::PendingApproval
            | StreamStatus::EscrowHold
    )
}

/// Per-stream sanity checks on a persisted [`Stream`].
pub fn assert_stream_invariants(stream: &Stream) {
    debug_assert!(
        stream.deposit >= 0,
        "stream {}: negative deposit {}",
        stream.id,
        stream.deposit
    );
    debug_assert!(
        stream.options.total_withdrawn >= 0,
        "stream {}: negative total_withdrawn {}",
        stream.id,
        stream.options.total_withdrawn
    );
    debug_assert!(
        stream.flow_rate >= 0,
        "stream {}: negative flow rate {}",
        stream.id,
        stream.flow_rate
    );
    debug_assert!(
        stream.start_time <= stream.end_time,
        "stream {}: start_time {} is after end_time {}",
        stream.id,
        stream.start_time,
        stream.end_time
    );
}

/// Contract-wide solvency check across every live stream, grouped by token.
pub fn assert_token_conservation(env: &Env) {
    let contract = env.current_contract_address();
    let count = get_global_stream_count(env);

    // Outstanding escrow per distinct token, accumulated as we walk the global
    // stream index. Two parallel vectors keep this dependency-free.
    let mut tokens: Vec<Address> = Vec::new(env);
    let mut owed: Vec<i128> = Vec::new(env);

    for idx in 0..count {
        let stream_id = match get_global_stream_at(env, idx) {
            Some(id) => id,
            None => continue,
        };
        let stream = match load_stream(env, stream_id) {
            Some(stream) => stream,
            None => continue,
        };
        if !holds_escrow(&stream.status) {
            continue;
        }

        // `saturating_sub` mirrors how the settlement paths compute the
        // remaining balance, so a rebased stream cannot make this negative.
        let remaining = stream
            .deposit
            .saturating_sub(stream.options.total_withdrawn);
        if remaining <= 0 {
            continue;
        }

        let mut found = false;
        for i in 0..tokens.len() {
            if tokens.get_unchecked(i) == stream.token {
                let total = owed.get_unchecked(i).saturating_add(remaining);
                let _ = owed.set(i, total);
                found = true;
                break;
            }
        }
        if !found {
            tokens.push_back(stream.token.clone());
            owed.push_back(remaining);
        }
    }

    for i in 0..tokens.len() {
        let token_address = tokens.get_unchecked(i);
        let required = owed.get_unchecked(i);
        let balance = token::Client::new(env, &token_address).balance(&contract);
        debug_assert!(
            balance >= required,
            "token conservation violated: contract holds {} but live streams owe {}",
            balance,
            required
        );
    }
}
