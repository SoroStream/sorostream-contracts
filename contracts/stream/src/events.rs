#![allow(dead_code)]
use soroban_sdk::{Address, Bytes, Env, String, Symbol};

/// Event schema version for compatibility tracking
const EVENT_SCHEMA_VERSION: u32 = 1;

/// Returns the current event schema version for off-chain compatibility checking
pub fn get_event_schema_version() -> u32 {
    EVENT_SCHEMA_VERSION
}

/// Emitted when a new stream is created.
pub fn stream_created(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    amount: i128,
    flow_rate: i128,
    end_time: u64,
    non_transferable: bool,
    comment: &Option<String>,
) {
    env.events().publish(
        (Symbol::new(env, "StreamCreated"), stream_id),
        (
            EVENT_SCHEMA_VERSION,
            sender.clone(),
            recipient.clone(),
            amount,
            flow_rate,
            end_time,
            non_transferable,
            comment.clone(),
        ),
    );
}

/// Emitted when a recipient withdraws claimable tokens.
///
/// `total_withdrawn` reflects the cumulative amount withdrawn from this stream
/// including the current withdrawal, computed after the stream state has been
/// updated (checks-effects-interactions order).
pub fn stream_withdrawn(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
    amount: i128,
    timestamp: u64,
    total_withdrawn: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamWithdrawn"), stream_id),
        (EVENT_SCHEMA_VERSION, recipient.clone(), amount, timestamp, total_withdrawn),
    );
}

/// Emitted when a sender cancels a stream.
pub fn stream_cancelled(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    refund_amount: i128,
    recipient_amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamCancelled"), stream_id),
        (EVENT_SCHEMA_VERSION, sender.clone(), refund_amount, recipient_amount),
    );
}

/// Emitted when a cancellation fee is deducted from the sender's refund.
///
/// `fee_amount`  — tokens withheld from the sender refund and sent to the treasury.
/// `fee_bps`     — the fee rate in basis points at the time of cancellation.
pub fn cancellation_fee_collected(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    fee_amount: i128,
    fee_bps: u32,
) {
    env.events().publish(
        (Symbol::new(env, "CancelFeeCollected"), stream_id),
        (sender.clone(), fee_amount, fee_bps),
    );
}

/// Emitted when a sender tops up an existing stream.
pub fn stream_topped_up(env: &Env, stream_id: u64, added_amount: i128, new_end_time: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamToppedUp"), stream_id),
        (added_amount, new_end_time),
    );
}

/// Emitted when a stream naturally reaches its end time.
pub fn stream_completed(env: &Env, stream_id: u64) {
    env.events()
        .publish((Symbol::new(env, "StreamCompleted"), stream_id), ());
}

/// Emitted when an auto-renew re-lock fails because the sender has insufficient balance.
pub fn auto_renew_failed(env: &Env, stream_id: u64, sender: &Address, required: i128) {
    env.events().publish(
        (Symbol::new(env, "AutoRenewFailed"), stream_id),
        (sender.clone(), required),
    );
}

/// Emitted when a stream's renewal count limit is reached and the stream can no longer auto-renew.
pub fn renewal_limit_reached(env: &Env, stream_id: u64, sender: &Address, renewals_used: u32) {
    env.events().publish(
        (Symbol::new(env, "RenewalLimitReached"), stream_id),
        (sender.clone(), renewals_used),
    );
}

/// Emitted when the contract is initialized with a version.
pub fn contract_deployed(env: &Env, version: &String, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "ContractDeployed"),),
        (EVENT_SCHEMA_VERSION, version.clone(), admin.clone()),
    );
}

/// Emitted when a sender partially cancels a stream, spawning a new smaller stream.
pub fn stream_partial_cancelled(
    env: &Env,
    old_stream_id: u64,
    new_stream_id: u64,
    sender: &Address,
    refund_amount: i128,
    new_deposit: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamPartialCancelled"), old_stream_id),
        (new_stream_id, sender.clone(), refund_amount, new_deposit),
    );
}

/// Emitted when the contract is paused during an emergency.
pub fn contract_paused(env: &Env, admin: &Address, timestamp: u64) {
    env.events().publish(
        (Symbol::new(env, "ContractPaused"), admin.clone()),
        timestamp,
    );
}

/// Emitted when the contract is resumed after an emergency pause.
pub fn contract_resumed(env: &Env, admin: &Address, timestamp: u64) {
    env.events().publish(
        (Symbol::new(env, "ContractResumed"), admin.clone()),
        timestamp,
    );
}

/// Emitted when a stream is paused by the sender.
pub fn stream_paused(env: &Env, stream_id: u64, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "StreamPaused"), stream_id),
        sender.clone(),
    );
}

/// Emitted when a stream is resumed by the sender.
pub fn stream_resumed(env: &Env, stream_id: u64, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "StreamResumed"), stream_id),
        sender.clone(),
    );
}

/// Emitted when a protocol fee is collected on withdrawal.
pub fn fee_collected(
    env: &Env,
    stream_id: u64,
    amount: i128,
    treasury: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "FeeCollected"), stream_id),
        (amount, treasury.clone()),
    );
}

/// Emitted when a fee change is proposed.
pub fn fee_change_proposed(env: &Env, new_fee: u32, unlock_time: u64) {
    env.events().publish(
        (Symbol::new(env, "FeeChangeProposed"),),
        (new_fee, unlock_time),
    );
}

/// Emitted when a fee change is executed.
pub fn fee_change_executed(env: &Env, new_fee: u32) {
    env.events().publish(
        (Symbol::new(env, "FeeChangeExecuted"),),
        (new_fee,),
    );
}

/// Emitted when a recipient terminates a stream early.
pub fn stream_terminated_by_recipient(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
    recipient_amount: i128,
    refund_amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamTerminatedByRecipient"), stream_id),
        (recipient.clone(), recipient_amount, refund_amount),
    );
}

/// Emitted when a stream recipient transfers their rights to a new recipient.
pub fn recipient_transferred(
    env: &Env,
    stream_id: u64,
    old_recipient: &Address,
    new_recipient: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "RecipientTransferred"), stream_id),
        (old_recipient.clone(), new_recipient.clone()),
    );
}

/// Emitted when a migration is successfully applied.
pub fn contract_migrated(env: &Env, from_version: &String, to_version: &String, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "ContractMigrated"),),
        (from_version.clone(), to_version.clone(), admin.clone()),
    );
}

/// Emitted when an admin action is logged.
pub fn admin_action(env: &Env, instruction: &String, admin: &Address, timestamp: u64) {
    env.events().publish(
        (Symbol::new(env, "AdminAction"),),
        (instruction.clone(), admin.clone(), timestamp),
    );
}

/// Emitted when a stream is archived after full settlement.
pub fn stream_archived(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    total_amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamArchived"), stream_id),
        (sender.clone(), recipient.clone(), total_amount),
    );
}

/// Emitted when metadata is updated for a stream.
pub fn metadata_updated(env: &Env, stream_id: u64, metadata: &Bytes) {
    env.events().publish(
        (Symbol::new(env, "MetadataUpdated"), stream_id),
        metadata.clone(),
    );
}

/// Emitted when a stream's metadata URI is updated.
pub fn metadata_uri_updated(env: &Env, stream_id: u64, metadata_uri: &Option<String>) {
    let uri_str = if let Some(uri) = metadata_uri {
        uri.clone()
    } else {
        String::from_str(env, "")
    };
    env.events().publish(
        (Symbol::new(env, "MetadataUriUpdated"), stream_id),
        uri_str,
    );
}

/// Emitted when an expired stream is swept from storage.
pub fn stream_swept(env: &Env, stream_id: u64, caller: &Address) {
    env.events().publish(
        (Symbol::new(env, "StreamSwept"), stream_id),
        caller.clone(),
    );
}

/// Emitted when a milestone is released by the sender.
pub fn milestone_released(env: &Env, stream_id: u64, milestone_index: u32) {
    env.events().publish(
        (Symbol::new(env, "MilestoneReleased"), stream_id),
        milestone_index,
    );
}

/// Emitted when an auto-renewal is cancelled for a stream.
pub fn auto_renew_cancelled(env: &Env, stream_id: u64) {
    env.events().publish(
        (Symbol::new(env, "AutoRenewCancelled"), stream_id),
        (),
    );
}

/// Emitted when a stream is renewed.
#[allow(dead_code)]
pub fn stream_renewed(env: &Env, old_stream_id: u64, new_stream_id: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamRenewed"), old_stream_id),
        new_stream_id,
    );
}

/// Emitted when a creation fee is collected in XLM at stream creation time.
pub fn creation_fee_collected(env: &Env, fee_amount: i128, treasury: &Address) {
    env.events().publish(
        (Symbol::new(env, "CreationFeeCollected"),),
        (fee_amount, treasury.clone()),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Escrow Hold Events
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted when a stream is created with escrow_hold = true.
///
/// # Event Data
/// - `stream_id`: The stream placed in escrow
/// - `sender`: The stream creator (who must activate it)
/// - `recipient`: The stream recipient
/// - `amount`: The amount locked in escrow
pub fn stream_placed_in_escrow(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamPlacedInEscrow"), stream_id),
        (sender.clone(), recipient.clone(), amount),
    );
}

/// Emitted when a stream is activated after being in escrow_hold state.
///
/// # Event Data
/// - `stream_id`: The activated stream
/// - `sender`: The sender who activated it
/// - `activation_timestamp`: Ledger timestamp of activation
pub fn stream_activated(env: &Env, stream_id: u64, sender: &Address, activation_timestamp: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamActivated"), stream_id),
        (sender.clone(), activation_timestamp),
    );
}

/// Emitted when accumulated protocol fees are swept from the contract to a destination.
/// Emitted when the sender releases the holdback escrow to the recipient.
pub fn holdback_released(env: &Env, stream_id: u64, amount: i128, recipient: &Address) {
    env.events().publish(
        (Symbol::new(env, "HoldbackReleased"), stream_id),
        (amount, recipient.clone()),
    );
}

/// Emitted when the sender claws back the holdback escrow before the recipient claims it.
pub fn holdback_clawed_back(env: &Env, stream_id: u64, amount: i128, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "HoldbackClawedBack"), stream_id),
        (amount, sender.clone()),
    );
}

// ---------------------------------------------------------------------------
// Step-vesting tranche events
// ---------------------------------------------------------------------------

/// Emitted when a step-vesting stream is created with a tranche schedule.
pub fn tranche_stream_created(env: &Env, stream_id: u64, sender: &Address, tranche_count: u32, total_amount: i128) {
    env.events().publish(
        (Symbol::new(env, "TrancheStreamCreated"), stream_id),
        (sender.clone(), tranche_count, total_amount),
    );
}

/// Emitted when one or more tranches are claimed during a withdrawal.
pub fn tranches_withdrawn(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
    tranches_claimed: u32,
    amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "TranchesWithdrawn"), stream_id),
        (recipient.clone(), tranches_claimed, amount),
    );
}

/// Emitted when a step-vesting stream is cancelled and unclaimed tranches are refunded.
pub fn tranche_stream_cancelled(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    unclaimed_tranche_refund: i128,
    recipient_amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "TrancheStreamCancelled"), stream_id),
        (sender.clone(), unclaimed_tranche_refund, recipient_amount),
    );
}

// ---------------------------------------------------------------------------
// Oracle price-check event
// ---------------------------------------------------------------------------

/// Emitted when an oracle price check passes successfully.
pub fn price_check_passed(
    env: &Env,
    stream_id: u64,
    token: &Address,
    price: i128,
    deviation_bps: u32,
) {
    env.events().publish(
        (Symbol::new(env, "PriceCheckPassed"), stream_id),
        (token.clone(), price, deviation_bps),
    );
}

/// Emitted when a stream transitions to the Expired state via mark_expired.
pub fn stream_expired(env: &Env, stream_id: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamExpired"), stream_id),
        (),
    );
}

/// Emitted when a stream's TTL is bumped to extend its ledger lifetime.
pub fn ttl_bumped(env: &Env, stream_id: u64, new_expiry_ledger: u32) {
    env.events().publish(
        (Symbol::new(env, "TtlBumped"), stream_id),
        new_expiry_ledger,
    );
}

/// Emitted when a delegate is set for a stream.
pub fn delegate_set(env: &Env, stream_id: u64, sender: &Address, delegate: &Address) {
    env.events().publish(
        (Symbol::new(env, "DelegateSet"), stream_id),
        (sender.clone(), delegate.clone()),
    );
}

/// Emitted when a delegate is revoked from a stream.
pub fn delegate_revoked(env: &Env, stream_id: u64, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "DelegateRevoked"), stream_id),
        sender.clone(),
    );
}
/// Emitted when fees are swept from the contract.
pub fn fee_swept(env: &Env, token: &Address, amount: i128, destination: &Address) {
    env.events().publish(
        (Symbol::new(env, "FeeSwept"),),
        (token.clone(), amount, destination.clone()),
    );
}

/// Emitted when slippage threshold is exceeded.
pub fn slippage_exceeded(env: &Env, stream_id: u64, current_price: i128, max_slippage_bps: u32) {
    env.events().publish(
        (Symbol::new(env, "SlippageExceeded"), stream_id),
        (current_price, max_slippage_bps),
    );
}

/// Emitted when slippage is within 80% of the limit (warning).
pub fn slippage_warning(env: &Env, stream_id: u64, current_deviation_bps: u32, max_slippage_bps: u32) {
    env.events().publish(
        (Symbol::new(env, "SlippageWarning"), stream_id),
        (current_deviation_bps, max_slippage_bps),
    );
}

/// Emitted when an address hits the rate limit.
pub fn rate_limit_exceeded(env: &Env, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "RateLimitExceeded"),),
        sender.clone(),
    );
}

/// Emitted when rate limit parameters are updated.
pub fn rate_limit_updated(env: &Env, window_seconds: u64, max_creations: u32) {
    env.events().publish(
        (Symbol::new(env, "RateLimitUpdated"),),
        (window_seconds, max_creations),
    );
}

/// Emitted when a token is added to the whitelist.
pub fn token_whitelisted(env: &Env, token: &Address) {
    env.events().publish(
        (Symbol::new(env, "TokenWhitelisted"),),
        token.clone(),
    );
}

/// Emitted when a token is removed from the whitelist.
pub fn token_dwhitelisted(env: &Env, token: &Address) {
    env.events().publish(
        (Symbol::new(env, "TokenDewhitelisted"),),
        token.clone(),
    );
}

/// Emitted when token whitelist is toggled.
pub fn token_whitelist_toggled(env: &Env, enabled: bool) {
    env.events().publish(
        (Symbol::new(env, "TokenWhitelistToggled"),),
        enabled,
    );
}

/// Emitted when a recipient is added to the allowlist.
pub fn recipient_allowlisted(env: &Env, recipient: &Address) {
    env.events().publish(
        (Symbol::new(env, "RecipientAllowlisted"),),
        recipient.clone(),
    );
}

/// Emitted when a recipient is removed from the allowlist.
pub fn recipient_disallowlisted(env: &Env, recipient: &Address) {
    env.events().publish(
        (Symbol::new(env, "RecipientDisallowlisted"),),
        recipient.clone(),
    );
}

/// Emitted when recipient allowlist is toggled globally.
pub fn recipient_allowlist_toggled(env: &Env, enabled: bool) {
    env.events().publish(
        (Symbol::new(env, "RecipientAllowlistToggled"),),
        enabled,
    );
}

/// Emitted when a stream is created with allowlist enforcement enabled.
pub fn stream_created_with_allowlist_enforcement(env: &Env, stream_id: u64, recipient: &Address) {
    env.events().publish(
        (Symbol::new(env, "StreamCreatedWithAllowlistEnforcement"), stream_id),
        recipient.clone(),
    );
}

/// Emitted when a federation name is registered (Issue #238).
pub fn federation_registered(env: &Env, federation_name: &String, stellar_address: &Address) {
    env.events().publish(
        (Symbol::new(env, "FederationRegistered"),),
        (federation_name.clone(), stellar_address.clone()),
    );
}

/// Emitted when a federation name is unregistered (Issue #238).
pub fn federation_unregistered(env: &Env, federation_name: &String) {
    env.events().publish(
        (Symbol::new(env, "FederationUnregistered"),),
        federation_name.clone(),
    );
}

// ---------------------------------------------------------------------------
// Withdrawal steps & minimum withdrawal amount events
// ---------------------------------------------------------------------------

/// Emitted alongside `StreamCreated` when a stream is configured with
/// `withdrawal_steps` or `min_withdrawal_amount` (or both).
///
/// Indexers that only listen to `StreamCreated` will still function; this
/// supplemental event carries the extra configuration for SDK clients that
/// want to surface step/floor information.
pub fn stream_config(
    env: &Env,
    stream_id: u64,
    withdrawal_steps: Option<u32>,
    min_withdrawal_amount: Option<i128>,
) {
    env.events().publish(
        (Symbol::new(env, "StreamConfig"), stream_id),
        (withdrawal_steps, min_withdrawal_amount),
    );
}

/// Emitted when a withdrawal step is completed.
///
/// `step_index` is the 1-based step number just completed (1 = first step).
/// `amount` is the tokens transferred to the recipient for this step.
pub fn withdrawal_step_completed(
    env: &Env,
    stream_id: u64,
    step_index: u32,
    total_steps: u32,
    amount: i128,
    recipient: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "WithdrawalStepCompleted"), stream_id),
        (step_index, total_steps, amount, recipient.clone()),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (a): StreamExpiryWarning
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when a stream is within the expiry warning window.
///
/// Allows indexers and wallets to build proactive notification systems without polling.
/// Emitted once per stream per expiry window during the first contract interaction
/// that occurs within the configurable ledger window before `end_time`.
///
/// # Event Data
/// - `stream_id`: The stream that is approaching expiry
/// - `sender`: The stream sender
/// - `recipient`: The stream recipient
/// - `remaining_balance`: The amount not yet withdrawn (deposit - total_withdrawn)
/// - `ledgers_until_expiry`: Number of ledgers remaining until end_time
pub fn stream_expiry_warning(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    remaining_balance: i128,
    ledgers_until_expiry: u32,
) {
    env.events().publish(
        (Symbol::new(env, "StreamExpiryWarning"), stream_id),
        (sender.clone(), recipient.clone(), remaining_balance, ledgers_until_expiry),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (b): Sender reputation cap
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when a sender crosses the promotion threshold.
///
/// After crossing the threshold, the sender is no longer subject to the
/// `new_sender_stream_cap` and can create streams without the lower cap.
///
/// # Event Data
/// - `sender`: The sender that has been promoted
/// - `lifetime_count`: Total number of streams this sender has ever created
/// - `threshold`: The promotion threshold that was crossed
pub fn sender_promoted(
    env: &Env,
    sender: &Address,
    lifetime_count: u32,
    threshold: u32,
) {
    env.events().publish(
        (Symbol::new(env, "SenderPromoted"),),
        (sender.clone(), lifetime_count, threshold),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (c): Stream redirect
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when a redirect target is set for a stream.
///
/// # Event Data
/// - `stream_id`: The source stream
/// - `target_stream_id`: The target stream that withdrawals will be redirected to
/// - `recipient`: The recipient (must be the same for both streams)
pub fn stream_redirect_set(
    env: &Env,
    stream_id: u64,
    target_stream_id: u64,
    recipient: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "StreamRedirectSet"), stream_id),
        (target_stream_id, recipient.clone()),
    );
}

/// Emitted when a redirect target is cleared for a stream.
///
/// # Event Data
/// - `stream_id`: The source stream
/// - `recipient`: The recipient who cleared the redirect
pub fn stream_redirect_cleared(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "StreamRedirectCleared"), stream_id),
        recipient.clone(),
    );
}

/// Emitted when a withdrawal is redirected to another stream.
///
/// # Event Data
/// - `source_stream_id`: The stream that initiated the withdrawal
/// - `target_stream_id`: The stream that received the top-up
/// - `amount`: The amount that was redirected (topped up into target)
/// - `recipient`: The recipient (same for both streams)
pub fn stream_redirected(
    env: &Env,
    source_stream_id: u64,
    target_stream_id: u64,
    amount: i128,
    recipient: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "StreamRedirected"), source_stream_id),
        (target_stream_id, amount, recipient.clone()),
    );
}

// ---------------------------------------------------------------------------
// Step-interval pure helper (no storage, no env needed)
// ---------------------------------------------------------------------------

/// Computes the length in seconds of each evenly-spaced withdrawal step.
///
/// Returns `None` if `steps` is 0 (division by zero guard).
///
/// ```text
/// step_interval = (end_time - start_time) / steps   (integer division)
/// ```
///
/// The *k*-th step (0-based) boundary is:
/// ```text
/// start_time + (k + 1) * step_interval
/// ```
pub fn compute_step_interval(start_time: u64, end_time: u64, steps: u32) -> Option<u64> {
    if steps == 0 {
        return None;
    }
    let duration = end_time.saturating_sub(start_time);
    Some(duration / steps as u64)
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (d): Dual-token streams
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when a dual-token stream is created.
///
/// # Event Data
/// - `stream_id`: The newly created stream
/// - `sender`: The stream sender
/// - `recipient`: The stream recipient
/// - `token1`: The first token address
/// - `amount1`: The first token amount
/// - `token2`: The second token address
/// - `amount2`: The second token amount
/// - `end_time`: When the stream ends
pub fn dual_stream_created(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    token1: &Address,
    amount1: i128,
    token2: &Address,
    amount2: i128,
    end_time: u64,
) {
    env.events().publish(
        (Symbol::new(env, "DualStreamCreated"), stream_id),
        (
            sender.clone(),
            recipient.clone(),
            token1.clone(),
            amount1,
            token2.clone(),
            amount2,
            end_time,
        ),
    );
}

/// Emitted when a dual-token stream withdrawal occurs.
///
/// # Event Data
/// - `stream_id`: The stream being withdrawn from
/// - `recipient`: The recipient who withdrew
/// - `amount1`: Amount withdrawn from token1
/// - `amount2`: Amount withdrawn from token2
/// - `timestamp`: Ledger timestamp of the withdrawal
pub fn dual_stream_withdrawn(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
    amount1: i128,
    amount2: i128,
    timestamp: u64,
) {
    env.events().publish(
        (Symbol::new(env, "DualStreamWithdrawn"), stream_id),
        (recipient.clone(), amount1, amount2, timestamp),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (h): Address blocklist events
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when an address is added to the blocklist.
pub fn address_blocked(env: &Env, admin: &Address, addr: &Address) {
    env.events().publish(
        (Symbol::new(env, "AddressBlocked"),),
        (admin.clone(), addr.clone()),
    );
}

/// Emitted when an address is removed from the blocklist.
pub fn address_unblocked(env: &Env, admin: &Address, addr: &Address) {
    env.events().publish(
        (Symbol::new(env, "AddressUnblocked"),),
        (admin.clone(), addr.clone()),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature (i): Post-expiry grace period events
// ═══════════════════════════════════════════════════════════════════════════

/// Emitted when a sender recovers expired stream funds after the grace period.
pub fn stream_recovered(env: &Env, stream_id: u64, sender: &Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "StreamRecovered"), stream_id),
        (sender.clone(), amount),
    );
}

/// Emitted when a dual-token stream is cancelled.
///
/// # Event Data
/// - `stream_id`: The cancelled stream
/// - `sender`: The sender who cancelled
/// - `refund_amount1`: Amount refunded to sender from token1
/// - `recipient_amount1`: Amount sent to recipient from token1
/// - `refund_amount2`: Amount refunded to sender from token2
/// - `recipient_amount2`: Amount sent to recipient from token2
pub fn dual_stream_cancelled(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    refund_amount1: i128,
    recipient_amount1: i128,
    refund_amount2: i128,
    recipient_amount2: i128,
) {
    env.events().publish(
        (Symbol::new(env, "DualStreamCancelled"), stream_id),
        (
            sender.clone(),
            refund_amount1,
            recipient_amount1,
            refund_amount2,
            recipient_amount2,
        ),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Recipient approval & sender lock events
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted when a recipient approves a stream that required explicit approval.
///
/// # Event Data
/// - `stream_id`: The approved stream
/// - `recipient`: The recipient who approved
/// - `approval_timestamp`: Ledger timestamp at which approval was recorded;
///   tokens begin accruing from this point
pub fn stream_approved(env: &Env, stream_id: u64, recipient: &Address, approval_timestamp: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamApproved"), stream_id),
        (recipient.clone(), approval_timestamp),
    );
}

// ── Issue #357: Stream inheritance ───────────────────────────────────────────

/// Emitted when stream inheritance triggers a new stream for the inherit recipient.
///
/// # Event Data
/// - `original_stream_id`: The completed or ended stream that triggered inheritance
/// - `new_stream_id`: The newly created stream for the inherit recipient
/// - `inherit_recipient`: The address that received the new stream
/// - `amount`: The amount forwarded to the inherit recipient
pub fn inheritance_triggered(
    env: &Env,
    original_stream_id: u64,
    new_stream_id: u64,
    inherit_recipient: &Address,
    amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "InheritanceTriggered"), original_stream_id),
        (new_stream_id, inherit_recipient.clone(), amount),
    );
}

// ── Issue #359: Fee exemption events ─────────────────────────────────────────

/// Emitted when an address is added to the fee exemption list.
pub fn fee_exemption_added(env: &Env, admin: &Address, addr: &Address) {
    env.events().publish(
        (Symbol::new(env, "FeeExemptionAdded"),),
        (admin.clone(), addr.clone()),
    );
}

/// Emitted when an address is removed from the fee exemption list.
pub fn fee_exemption_removed(env: &Env, admin: &Address, addr: &Address) {
    env.events().publish(
        (Symbol::new(env, "FeeExemptionRemoved"),),
        (admin.clone(), addr.clone()),
    );
}

/// Emitted when a sender irrevocably locks a stream, renouncing their right to cancel.
///
/// # Event Data
/// - `stream_id`: The locked stream
/// - `sender`: The sender who initiated the lock
pub fn stream_sender_locked(env: &Env, stream_id: u64, sender: &Address) {
    env.events().publish(
        (Symbol::new(env, "StreamSenderLocked"), stream_id),
        sender.clone(),
    );
}


/// Emitted when a sender updates the flow rate of an active stream.
///
/// # Event Data
/// - `stream_id`: The stream whose rate was updated
/// - `old_rate`: The previous tokens-per-second flow rate
/// - `new_rate`: The new tokens-per-second flow rate
/// - `new_end_time`: The adjusted stream end time after rate change
/// - `remaining_deposit`: The remaining deposit after balance settlement
pub fn stream_rate_updated(
    env: &Env,
    stream_id: u64,
    old_rate: i128,
    new_rate: i128,
    new_end_time: u64,
    remaining_deposit: i128,
) {
    env.events().publish(
        (Symbol::new(env, "StreamRateUpdated"), stream_id),
        (old_rate, new_rate, new_end_time, remaining_deposit),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Split Stream Events
// ─────────────────────────────────────────────────────────────────────────────

use soroban_sdk::Vec;

/// Emitted when a split stream is created with multiple recipients.
///
/// A split stream distributes a single deposit across N sub-streams, each
/// with a proportional allocation based on basis points.
///
/// # Event Data
/// - `split_stream_id`: Unique identifier for the split stream
/// - `sender`: The split stream creator / payer
/// - `total_deposit`: Total amount distributed across all recipients
/// - `stream_ids`: Vector of sub-stream IDs (one per recipient, in order)
/// - `recipients`: Vector of recipient addresses (parallel to stream_ids)
/// - `weights_bps`: Vector of weights in basis points (parallel to recipients)
/// - `token`: Token address used for all sub-streams
/// - `duration_seconds`: Duration in seconds for each sub-stream
pub fn split_stream_created(
    env: &Env,
    split_stream_id: u64,
    sender: &Address,
    total_deposit: i128,
    stream_ids: &Vec<u64>,
    recipients: &Vec<Address>,
    weights_bps: &Vec<u16>,
    token: &Address,
    duration_seconds: u64,
) {
    env.events().publish(
        (Symbol::new(env, "SplitStreamCreated"), split_stream_id),
        (
            sender.clone(),
            total_deposit,
            stream_ids.clone(),
            recipients.clone(),
            weights_bps.clone(),
            token.clone(),
            duration_seconds,
        ),
    );
}


/// Emitted when an admin sweeps a dormant stream.
///
/// A dormant stream is one that has not received withdrawals for longer than
/// the configured dormancy threshold. Admin can sweep these to reclaim capital
/// and storage.
///
/// # Event Data
/// - `stream_id`: The swept stream
/// - `sender`: The stream creator (who receives the refund)
/// - `refund_amount`: Amount refunded to sender (remaining deposit)
/// - `last_withdraw_time`: Last time tokens were withdrawn from this stream
pub fn dormant_stream_cancelled(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    refund_amount: i128,
    last_withdraw_time: u64,
) {
    env.events().publish(
        (Symbol::new(env, "DormantStreamCancelled"), stream_id),
        (sender.clone(), refund_amount, last_withdraw_time),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// On-Complete Callback Events
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted when an on_complete callback is invoked when a stream reaches its end_time.
///
/// # Event Data
/// - `stream_id`: The stream that completed
/// - `on_complete_contract`: The contract address that was invoked
/// - `on_complete_function`: The function name that was called
pub fn on_complete_invoked(
    env: &Env,
    stream_id: u64,
    on_complete_contract: &Address,
    on_complete_function: &Symbol,
) {
    env.events().publish(
        (Symbol::new(env, "OnCompleteInvoked"), stream_id),
        (on_complete_contract.clone(), on_complete_function.clone()),
    );
}

/// Emitted when an on_complete callback execution succeeds.
///
/// # Event Data
/// - `stream_id`: The stream that completed
/// - `on_complete_contract`: The contract address that was invoked
pub fn on_complete_success(env: &Env, stream_id: u64, on_complete_contract: &Address) {
    env.events().publish(
        (Symbol::new(env, "OnCompleteSuccess"), stream_id),
        on_complete_contract.clone(),
    );
}

/// Emitted when an on_complete callback execution fails.
///
/// # Event Data
/// - `stream_id`: The stream that completed
/// - `on_complete_contract`: The contract address that was invoked
/// - `error_message`: Description of the error that occurred
pub fn on_complete_failed(
    env: &Env,
    stream_id: u64,
    on_complete_contract: &Address,
    error_message: &String,
) {
    env.events().publish(
        (Symbol::new(env, "OnCompleteFailed"), stream_id),
        (on_complete_contract.clone(), error_message.clone()),
    );
}

// ── Issue #292: Role-based admin access control events ──────────────────────

/// Emitted when the super-admin assigns a new role to an address.
///
/// `role_name` is a string label (e.g. `"FeeManager"`, `"EmergencyPause"`, `"Analytics"`).
pub fn role_assigned(env: &Env, role_name: &String, assignee: &Address, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "RoleAssigned"),),
        (role_name.clone(), assignee.clone(), admin.clone()),
    );
}

/// Emitted when the super-admin revokes a role from an address.
pub fn role_revoked(env: &Env, role_name: &String, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "RoleRevoked"),),
        (role_name.clone(), admin.clone()),
    );
}

// ── Issue #293: Sender collateral / stake events ─────────────────────────────

/// Emitted when a sender deposits stake collateral.
pub fn stake_deposited(env: &Env, sender: &Address, token: &Address, amount: i128, new_balance: i128) {
    env.events().publish(
        (Symbol::new(env, "StakeDeposited"),),
        (sender.clone(), token.clone(), amount, new_balance),
    );
}

/// Emitted when a sender initiates an unstake (starts the cooldown period).
pub fn unstake_initiated(env: &Env, sender: &Address, token: &Address, amount: i128, unlock_at: u64) {
    env.events().publish(
        (Symbol::new(env, "UnstakeInitiated"),),
        (sender.clone(), token.clone(), amount, unlock_at),
    );
}

/// Emitted when a sender completes an unstake after the cooldown period.
pub fn unstake_completed(env: &Env, sender: &Address, token: &Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "UnstakeCompleted"),),
        (sender.clone(), token.clone(), amount),
    );
}

/// Emitted when the admin slashes a sender's staked collateral.
pub fn stake_slashed(
    env: &Env,
    sender: &Address,
    token: &Address,
    slash_amount: i128,
    destination: &Address,
    admin: &Address,
) {
    env.events().publish(
        (Symbol::new(env, "StakeSlashed"),),
        (sender.clone(), token.clone(), slash_amount, destination.clone(), admin.clone()),
    );
}

/// Emitted when the admin sets a new minimum stake threshold for a token.
pub fn min_stake_set(env: &Env, token: &Address, amount: i128, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "MinStakeSet"),),
        (token.clone(), amount, admin.clone()),
    );
}
