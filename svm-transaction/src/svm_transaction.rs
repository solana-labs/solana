use {crate::svm_message::SVMMessage, solana_sdk::signature::Signature};

mod sanitized_transaction;

pub trait SVMTransaction: SVMMessage {
    /// Get the first signature of the message.
    fn signature(&self) -> &Signature;

    /// Get all the signatures of the message.
    fn signatures(&self) -> &[Signature];
}
