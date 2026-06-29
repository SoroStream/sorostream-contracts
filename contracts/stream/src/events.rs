use soroban_sdk::{Address, Bytes, Env, String, Symbol};

/// Emitted when a new stream is created.
pub fn stream_created(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    amount: i128,
    flow_rate: i128,
    end_time: u64,
) {
    env.events().publish(
        (Symbol::new(env, "StreamCreated"), stream_id),
        (
            sender.clone(),
            recipient.clone(),
            amount,
            flow_rate,
            end_time,
        ),
    );
}

/// Emitted when a recipient withdraws claimable tokens.
pub fn stream_withdrawn(
    env: &Env,
    stream_id: u64,
    recipient: &Address,
    amount: i128,
    timestamp: u64,
) {
    env.events().publish(
        (Symbol::new(env, "StreamWithdrawn"), stream_id),
        (recipient.clone(), amount, timestamp),
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
        (sender.clone(), refund_amount, recipient_amount),
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

/// Emitted when the contract is initialized with a version.
pub fn contract_deployed(env: &Env, version: &String, admin: &Address) {
    env.events().publish(
        (Symbol::new(env, "ContractDeployed"),),
        (version.clone(), admin.clone()),
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
/// Emitted when metadata is updated for a stream.
pub fn metadata_updated(env: &Env, stream_id: u64, metadata: &Bytes) {
    env.events().publish(
        (Symbol::new(env, "MetadataUpdated"), stream_id),
        metadata.clone(),
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
pub fn stream_renewed(env: &Env, old_stream_id: u64, new_stream_id: u64) {
    env.events().publish(
        (Symbol::new(env, "StreamRenewed"), old_stream_id),
        new_stream_id,
    );
}
