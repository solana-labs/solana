use {
    crate::banking_stage::{
        immutable_deserialized_packet::ImmutableDeserializedPacket, scheduler_messages::MaxAge,
    },
    solana_sdk::transaction::SanitizedTransaction,
    std::sync::Arc,
};

/// Simple wrapper type to tie a sanitized transaction to max age slot.
pub(crate) struct SanitizedTransactionTTL {
    pub(crate) transaction: SanitizedTransaction,
    pub(crate) max_age: MaxAge,
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
        packet: Arc<ImmutableDeserializedPacket>,
        priority: u64,
        cost: u64,
        should_forward: bool,
    },
    /// The transaction is currently scheduled or being processed.
    Pending {
        packet: Arc<ImmutableDeserializedPacket>,
        priority: u64,
        cost: u64,
        should_forward: bool,
    },
    /// Only used during transition.
    Transitioning,
}

impl TransactionState {
    /// Creates a new `TransactionState` in the `Unprocessed` state.
    pub(crate) fn new(
        transaction_ttl: SanitizedTransactionTTL,
        packet: Arc<ImmutableDeserializedPacket>,
        priority: u64,
        cost: u64,
    ) -> Self {
        let should_forward = !packet.original_packet().meta().forwarded()
            && packet.original_packet().meta().is_from_staked_node();
        Self::Unprocessed {
            transaction_ttl,
            packet,
            priority,
            cost,
            should_forward,
        }
    }

    /// Return the priority of the transaction.
    /// This is *not* the same as the `compute_unit_price` of the transaction.
    /// The priority is used to order transactions for processing.
    pub(crate) fn priority(&self) -> u64 {
        match self {
            Self::Unprocessed { priority, .. } => *priority,
            Self::Pending { priority, .. } => *priority,
            Self::Transitioning => unreachable!(),
        }
    }

    /// Return the cost of the transaction.
    pub(crate) fn cost(&self) -> u64 {
        match self {
            Self::Unprocessed { cost, .. } => *cost,
            Self::Pending { cost, .. } => *cost,
            Self::Transitioning => unreachable!(),
        }
    }

    /// Return whether packet should be attempted to be forwarded.
    pub(crate) fn should_forward(&self) -> bool {
        match self {
            Self::Unprocessed {
                should_forward: forwarded,
                ..
            } => *forwarded,
            Self::Pending {
                should_forward: forwarded,
                ..
            } => *forwarded,
            Self::Transitioning => unreachable!(),
        }
    }

    /// Mark the packet as forwarded.
    /// This is used to prevent the packet from being forwarded multiple times.
    pub(crate) fn mark_forwarded(&mut self) {
        match self {
            Self::Unprocessed { should_forward, .. } => *should_forward = false,
            Self::Pending { should_forward, .. } => *should_forward = false,
            Self::Transitioning => unreachable!(),
        }
    }

    /// Return the packet of the transaction.
    pub(crate) fn packet(&self) -> &Arc<ImmutableDeserializedPacket> {
        match self {
            Self::Unprocessed { packet, .. } => packet,
            Self::Pending { packet, .. } => packet,
            Self::Transitioning => unreachable!(),
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
                packet,
                priority,
                cost,
                should_forward: forwarded,
            } => {
                *self = TransactionState::Pending {
                    packet,
                    priority,
                    cost,
                    should_forward: forwarded,
                };
                transaction_ttl
            }
            TransactionState::Pending { .. } => {
                panic!("transaction already pending");
            }
            Self::Transitioning => unreachable!(),
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
                packet,
                priority,
                cost,
                should_forward: forwarded,
            } => {
                *self = Self::Unprocessed {
                    transaction_ttl,
                    packet,
                    priority,
                    cost,
                    should_forward: forwarded,
                }
            }
            Self::Transitioning => unreachable!(),
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
            Self::Transitioning => unreachable!(),
        }
    }

    /// Internal helper to transitioning between states.
    /// Replaces `self` with a dummy state that will immediately be overwritten in transition.
    fn take(&mut self) -> Self {
        core::mem::replace(self, Self::Transitioning)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            clock::Slot, compute_budget::ComputeBudgetInstruction, hash::Hash, message::Message,
            packet::Packet, signature::Keypair, signer::Signer, system_instruction,
            transaction::Transaction,
        },
    };

    fn create_transaction_state(compute_unit_price: u64) -> TransactionState {
        let from_keypair = Keypair::new();
        let ixs = vec![
            system_instruction::transfer(
                &from_keypair.pubkey(),
                &solana_sdk::pubkey::new_rand(),
                1,
            ),
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ];
        let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
        let tx = Transaction::new(&[&from_keypair], message, Hash::default());

        let packet = Arc::new(
            ImmutableDeserializedPacket::new(Packet::from_data(None, tx.clone()).unwrap()).unwrap(),
        );
        let transaction_ttl = SanitizedTransactionTTL {
            transaction: SanitizedTransaction::from_transaction_for_tests(tx),
            max_age: MaxAge {
                epoch_invalidation_slot: Slot::MAX,
                alt_invalidation_slot: Slot::MAX,
            },
        };
        const TEST_TRANSACTION_COST: u64 = 5000;
        TransactionState::new(
            transaction_ttl,
            packet,
            compute_unit_price,
            TEST_TRANSACTION_COST,
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
            max_age,
        } = transaction_state.transaction_ttl();
        let transaction_ttl = SanitizedTransactionTTL {
            transaction: transaction.clone(),
            max_age: *max_age,
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
    fn test_priority() {
        let priority = 15;
        let mut transaction_state = create_transaction_state(priority);
        assert_eq!(transaction_state.priority(), priority);

        // ensure compute unit price is not lost through state transitions
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
        assert_eq!(
            transaction_ttl.max_age,
            MaxAge {
                epoch_invalidation_slot: Slot::MAX,
                alt_invalidation_slot: Slot::MAX,
            }
        );

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
        assert_eq!(
            transaction_ttl.max_age,
            MaxAge {
                epoch_invalidation_slot: Slot::MAX,
                alt_invalidation_slot: Slot::MAX,
            }
        );

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
        assert_eq!(
            transaction_ttl.max_age,
            MaxAge {
                epoch_invalidation_slot: Slot::MAX,
                alt_invalidation_slot: Slot::MAX,
            }
        );
    }
}
