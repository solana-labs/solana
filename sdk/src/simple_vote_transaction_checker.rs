#![cfg(feature = "full")]

use {
    crate::{message::VersionedMessage, transaction::SanitizedVersionedTransaction},
    solana_pubkey::Pubkey,
    solana_signature::Signature,
};

/// Simple vote transaction meets these conditions:
/// 1. has 1 or 2 signatures;
/// 2. is legacy message;
/// 3. has only one instruction;
/// 4. which must be Vote instruction;
pub fn is_simple_vote_transaction(
    sanitized_versioned_transaction: &SanitizedVersionedTransaction,
) -> bool {
    let is_legacy_message = matches!(
        sanitized_versioned_transaction.message.message,
        VersionedMessage::Legacy(_)
    );
    let instruction_programs = sanitized_versioned_transaction
        .message
        .program_instructions_iter()
        .map(|(program_id, _ix)| program_id);

    is_simple_vote_transaction_impl(
        &sanitized_versioned_transaction.signatures,
        is_legacy_message,
        instruction_programs,
    )
}

/// Simple vote transaction meets these conditions:
/// 1. has 1 or 2 signatures;
/// 2. is legacy message;
/// 3. has only one instruction;
/// 4. which must be Vote instruction;
#[inline]
pub fn is_simple_vote_transaction_impl<'a>(
    signatures: &[Signature],
    is_legacy_message: bool,
    mut instruction_programs: impl Iterator<Item = &'a Pubkey>,
) -> bool {
    signatures.len() < 3
        && is_legacy_message
        && instruction_programs
            .next()
            .xor(instruction_programs.next())
            .map(|program_id| program_id == &solana_sdk::vote::program::ID)
            .unwrap_or(false)
}
