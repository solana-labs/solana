#![cfg(feature = "full")]

use crate::{message::VersionedMessage, transaction::SanitizedVersionedTransaction};

/// Simple vote transaction meets these conditions:
/// 1. has 1 or 2 signatures;
/// 2. is legacy message;
/// 3. has only one instruction;
/// 4. which must be Vote instruction;
pub fn is_simple_vote_transaction(
    sanitized_versioned_transaction: &SanitizedVersionedTransaction,
) -> bool {
    let signatures_count = sanitized_versioned_transaction.signatures.len();
    let is_legacy_message = matches!(
        sanitized_versioned_transaction.message.message,
        VersionedMessage::Legacy(_)
    );
    let mut instructions = sanitized_versioned_transaction
        .message
        .program_instructions_iter();
    signatures_count < 3
        && is_legacy_message
        && instructions
            .next()
            .xor(instructions.next())
            .map(|(program_id, _ix)| program_id == &solana_sdk::vote::program::id())
            .unwrap_or(false)
}
