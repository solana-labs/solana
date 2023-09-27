use {
    solana_runtime::transaction_priority_details::TransactionPriorityDetails,
    solana_sdk::{slot_history::Slot, transaction::SanitizedTransaction},
};

/// Simple wrapper type to tie a sanitized transaction to max age slot.
pub(crate) struct SanitizedTransactionTTL {
    pub(crate) transaction: SanitizedTransaction,
    pub(crate) max_age_slot: Slot,
}

/// TransactionState is used to track the state of a transaction in the transaction scheduler
/// and banking stage as a whole.
///
/// There are two states a transaction can be in:
///     1. `Unprocessed` - The transaction is available for scheduling.
///     2. `Pending` - The transaction is currently scheduled or being processed.
///
/// Newly received transactions are initially in the `Unprocessed` state.
/// When a transaction is scheduled, it is transitioned to the `Pending` state,
///   using the `transition_to_pending` method.
/// When a transaction finishes processing it may be retryable. If it is retryable,
///   the transaction is transitioned back to the `Unprocessed` state using the
///   `transition_to_unprocessed` method. If it is not retryable, the state should
///   be dropped.
///
/// For performance, when a transaction is transitioned to the `Pending` state, the
///   internal `SanitizedTransaction` is moved out of the `TransactionState` and sent
///   to the appropriate thread for processing. This is done to avoid cloning the
///  `SanitizedTransaction`.
#[allow(clippy::large_enum_variant)]
pub(crate) enum TransactionState {
    /// The transaction is available for scheduling.
    Unprocessed {
        transaction_ttl: SanitizedTransactionTTL,
        transaction_priority_details: TransactionPriorityDetails,
        forwarded: bool,
    },
    /// The transaction is currently scheduled or being processed.
    Pending {
        transaction_priority_details: TransactionPriorityDetails,
        forwarded: bool,
    },
}

impl TransactionState {
    /// Creates a new `TransactionState` in the `Unprocessed` state.
    pub(crate) fn new(
        transaction_ttl: SanitizedTransactionTTL,
        transaction_priority_details: TransactionPriorityDetails,
    ) -> Self {
        Self::Unprocessed {
            transaction_ttl,
            transaction_priority_details,
            forwarded: false,
        }
    }

    /// Returns a reference to the priority details of the transaction.
    pub(crate) fn transaction_priority_details(&self) -> &TransactionPriorityDetails {
        match self {
            Self::Unprocessed {
                transaction_priority_details,
                ..
            } => transaction_priority_details,
            Self::Pending {
                transaction_priority_details,
                ..
            } => transaction_priority_details,
        }
    }

    /// Returns the priority of the transaction.
    pub(crate) fn priority(&self) -> u64 {
        self.transaction_priority_details().priority
    }

    /// Returns whether or not the transaction has already been forwarded.
    pub(crate) fn forwarded(&self) -> bool {
        match self {
            Self::Unprocessed { forwarded, .. } => *forwarded,
            Self::Pending { forwarded, .. } => *forwarded,
        }
    }

    /// Sets the transaction as forwarded.
    pub(crate) fn set_forwarded(&mut self) {
        match self {
            Self::Unprocessed { forwarded, .. } => *forwarded = true,
            Self::Pending { forwarded, .. } => *forwarded = true,
        }
    }

    /// Intended to be called when a transaction is scheduled. This method will
    /// transition the transaction from `Unprocessed` to `Pending` and return the
    /// `SanitizedTransactionTTL` for processing.
    ///
    /// # Panics
    /// This method will panic if the transaction is already in the `Pending` state,
    ///   as this is an invalid state transition.
    pub(crate) fn transition_to_pending(&mut self) -> SanitizedTransactionTTL {
        match self.take() {
            TransactionState::Unprocessed {
                transaction_ttl,
                transaction_priority_details,
                forwarded,
            } => {
                *self = TransactionState::Pending {
                    transaction_priority_details,
                    forwarded,
                };
                transaction_ttl
            }
            TransactionState::Pending { .. } => {
                panic!("transaction already pending");
            }
        }
    }

    /// Intended to be called when a transaction is retried. This method will
    /// transition the transaction from `Pending` to `Unprocessed`.
    ///
    /// # Panics
    /// This method will panic if the transaction is already in the `Unprocessed`
    ///   state, as this is an invalid state transition.
    pub(crate) fn transition_to_unprocessed(&mut self, transaction_ttl: SanitizedTransactionTTL) {
        match self.take() {
            TransactionState::Unprocessed { .. } => panic!("already unprocessed"),
            TransactionState::Pending {
                transaction_priority_details,
                forwarded,
            } => {
                *self = Self::Unprocessed {
                    transaction_ttl,
                    transaction_priority_details,
                    forwarded,
                }
            }
        }
    }

    /// Get a reference to the `SanitizedTransactionTTL` for the transaction.
    ///
    /// # Panics
    /// This method will panic if the transaction is in the `Pending` state.
    pub(crate) fn transaction_ttl(&self) -> &SanitizedTransactionTTL {
        match self {
            Self::Unprocessed {
                transaction_ttl, ..
            } => transaction_ttl,
            Self::Pending { .. } => panic!("transaction is pending"),
        }
    }

    /// Internal helper to transitioning between states.
    /// Replaces `self` with a dummy state that will immediately be overwritten in transition.
    fn take(&mut self) -> Self {
        core::mem::replace(
            self,
            Self::Pending {
                transaction_priority_details: TransactionPriorityDetails {
                    priority: 0,
                    compute_unit_limit: 0,
                },
                forwarded: false,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, hash::Hash, message::Message,
            signature::Keypair, signer::Signer, system_instruction, transaction::Transaction,
        },
    };

    fn create_transaction_state(priority: u64) -> TransactionState {
        let from_keypair = Keypair::new();
        let ixs = vec![
            system_instruction::transfer(
                &from_keypair.pubkey(),
                &solana_sdk::pubkey::new_rand(),
                1,
            ),
            ComputeBudgetInstruction::set_compute_unit_price(priority),
        ];
        let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
        let tx = Transaction::new(&[&from_keypair], message, Hash::default());

        let transaction_ttl = SanitizedTransactionTTL {
            transaction: SanitizedTransaction::from_transaction_for_tests(tx),
            max_age_slot: Slot::MAX,
        };

        TransactionState::new(
            transaction_ttl,
            TransactionPriorityDetails {
                priority,
                compute_unit_limit: 0,
            },
        )
    }

    #[test]
    #[should_panic(expected = "already pending")]
    fn test_transition_to_pending_panic() {
        let mut transaction_state = create_transaction_state(0);
        transaction_state.transition_to_pending();
        transaction_state.transition_to_pending(); // invalid transition
    }

    #[test]
    fn test_transition_to_pending() {
        let mut transaction_state = create_transaction_state(0);
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
        let _ = transaction_state.transition_to_pending();
        assert!(matches!(
            transaction_state,
            TransactionState::Pending { .. }
        ));
    }

    #[test]
    #[should_panic(expected = "already unprocessed")]
    fn test_transition_to_unprocessed_panic() {
        let mut transaction_state = create_transaction_state(0);

        // Manually clone `SanitizedTransactionTTL`
        let SanitizedTransactionTTL {
            transaction,
            max_age_slot,
        } = transaction_state.transaction_ttl();
        let transaction_ttl = SanitizedTransactionTTL {
            transaction: transaction.clone(),
            max_age_slot: *max_age_slot,
        };
        transaction_state.transition_to_unprocessed(transaction_ttl); // invalid transition
    }

    #[test]
    fn test_transition_to_unprocessed() {
        let mut transaction_state = create_transaction_state(0);
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
        let transaction_ttl = transaction_state.transition_to_pending();
        assert!(matches!(
            transaction_state,
            TransactionState::Pending { .. }
        ));
        transaction_state.transition_to_unprocessed(transaction_ttl);
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
    }

    #[test]
    fn test_transaction_priority_details() {
        let priority = 15;
        let mut transaction_state = create_transaction_state(priority);
        assert_eq!(transaction_state.priority(), priority);

        // ensure priority is not lost through state transitions
        let transaction_ttl = transaction_state.transition_to_pending();
        assert_eq!(transaction_state.priority(), priority);
        transaction_state.transition_to_unprocessed(transaction_ttl);
        assert_eq!(transaction_state.priority(), priority);
    }

    #[test]
    #[should_panic(expected = "transaction is pending")]
    fn test_transaction_ttl_panic() {
        let mut transaction_state = create_transaction_state(0);
        let transaction_ttl = transaction_state.transaction_ttl();
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
        assert_eq!(transaction_ttl.max_age_slot, Slot::MAX);

        let _ = transaction_state.transition_to_pending();
        assert!(matches!(
            transaction_state,
            TransactionState::Pending { .. }
        ));
        let _ = transaction_state.transaction_ttl(); // pending state, the transaction ttl is not available
    }

    #[test]
    fn test_transaction_ttl() {
        let mut transaction_state = create_transaction_state(0);
        let transaction_ttl = transaction_state.transaction_ttl();
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
        assert_eq!(transaction_ttl.max_age_slot, Slot::MAX);

        // ensure transaction_ttl is not lost through state transitions
        let transaction_ttl = transaction_state.transition_to_pending();
        assert!(matches!(
            transaction_state,
            TransactionState::Pending { .. }
        ));

        transaction_state.transition_to_unprocessed(transaction_ttl);
        let transaction_ttl = transaction_state.transaction_ttl();
        assert!(matches!(
            transaction_state,
            TransactionState::Unprocessed { .. }
        ));
        assert_eq!(transaction_ttl.max_age_slot, Slot::MAX);
    }
}
