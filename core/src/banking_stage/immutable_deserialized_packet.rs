use {
    solana_cost_model::block_cost_limits::BUILT_IN_INSTRUCTION_COSTS,
    solana_perf::packet::Packet,
    solana_runtime::transaction_priority_details::{
        GetTransactionPriorityDetails, TransactionPriorityDetails,
    },
    solana_sdk::{
        feature_set,
        hash::Hash,
        message::Message,
        sanitize::SanitizeError,
        saturating_add_assign,
        short_vec::decode_shortu16_len,
        signature::Signature,
        transaction::{
            AddressLoader, SanitizedTransaction, SanitizedVersionedTransaction,
            VersionedTransaction,
        },
    },
    std::{cmp::Ordering, mem::size_of, sync::Arc},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum DeserializedPacketError {
    #[error("ShortVec Failed to Deserialize")]
    // short_vec::decode_shortu16_len() currently returns () on error
    ShortVecError(()),
    #[error("Deserialization Error: {0}")]
    DeserializationError(#[from] bincode::Error),
    #[error("overflowed on signature size {0}")]
    SignatureOverflowed(usize),
    #[error("packet failed sanitization {0}")]
    SanitizeError(#[from] SanitizeError),
    #[error("transaction failed prioritization")]
    PrioritizationFailure,
    #[error("vote transaction failure")]
    VoteTransactionError,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImmutableDeserializedPacket {
    original_packet: Packet,
    transaction: SanitizedVersionedTransaction,
    message_hash: Hash,
    is_simple_vote: bool,
    priority_details: TransactionPriorityDetails,
}

impl ImmutableDeserializedPacket {
    pub fn new(packet: Packet) -> Result<Self, DeserializedPacketError> {
        let versioned_transaction: VersionedTransaction = packet.deserialize_slice(..)?;
        let sanitized_transaction = SanitizedVersionedTransaction::try_from(versioned_transaction)?;
        let message_bytes = packet_message(&packet)?;
        let message_hash = Message::hash_raw_message(message_bytes);
        let is_simple_vote = packet.meta().is_simple_vote_tx();

        // drop transaction if prioritization fails.
        let mut priority_details = sanitized_transaction
            .get_transaction_priority_details(packet.meta().round_compute_unit_price())
            .ok_or(DeserializedPacketError::PrioritizationFailure)?;

        // set priority to zero for vote transactions
        if is_simple_vote {
            priority_details.priority = 0;
        };

        Ok(Self {
            original_packet: packet,
            transaction: sanitized_transaction,
            message_hash,
            is_simple_vote,
            priority_details,
        })
    }

    pub fn original_packet(&self) -> &Packet {
        &self.original_packet
    }

    pub fn transaction(&self) -> &SanitizedVersionedTransaction {
        &self.transaction
    }

    pub fn message_hash(&self) -> &Hash {
        &self.message_hash
    }

    pub fn is_simple_vote(&self) -> bool {
        self.is_simple_vote
    }

    pub fn priority(&self) -> u64 {
        self.priority_details.priority
    }

    pub fn compute_unit_limit(&self) -> u64 {
        self.priority_details.compute_unit_limit
    }

    /// Returns true if the transaction's compute unit limit is at least as
    /// large as the sum of the static builtins' costs.
    /// This is a simple sanity check so the leader can discard transactions
    /// which are statically known to exceed the compute budget, and will
    /// result in no useful state-change.
    pub fn compute_unit_limit_above_static_builtins(&self) -> bool {
        let mut static_builtin_cost_sum: u64 = 0;
        for (program_id, _) in self.transaction.get_message().program_instructions_iter() {
            if let Some(ix_cost) = BUILT_IN_INSTRUCTION_COSTS.get(program_id) {
                saturating_add_assign!(static_builtin_cost_sum, *ix_cost);
            }
        }

        self.compute_unit_limit() >= static_builtin_cost_sum
    }

    // This function deserializes packets into transactions, computes the blake3 hash of transaction
    // messages, and verifies secp256k1 instructions.
    pub fn build_sanitized_transaction(
        &self,
        feature_set: &Arc<feature_set::FeatureSet>,
        votes_only: bool,
        address_loader: impl AddressLoader,
    ) -> Option<SanitizedTransaction> {
        if votes_only && !self.is_simple_vote() {
            return None;
        }
        let tx = SanitizedTransaction::try_new(
            self.transaction().clone(),
            *self.message_hash(),
            self.is_simple_vote(),
            address_loader,
        )
        .ok()?;
        tx.verify_precompiles(feature_set).ok()?;
        Some(tx)
    }
}

impl PartialOrd for ImmutableDeserializedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ImmutableDeserializedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority().cmp(&other.priority())
    }
}

/// Read the transaction message from packet data
fn packet_message(packet: &Packet) -> Result<&[u8], DeserializedPacketError> {
    let (sig_len, sig_size) = packet
        .data(..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(DeserializedPacketError::ShortVecError(()))?;
    sig_len
        .checked_mul(size_of::<Signature>())
        .and_then(|v| v.checked_add(sig_size))
        .and_then(|msg_start| packet.data(msg_start..))
        .ok_or(DeserializedPacketError::SignatureOverflowed(sig_size))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget, instruction::Instruction, pubkey::Pubkey, signature::Keypair,
            signer::Signer, system_instruction, system_transaction, transaction::Transaction,
        },
    };

    #[test]
    fn simple_deserialized_packet() {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, tx).unwrap();
        let deserialized_packet = ImmutableDeserializedPacket::new(packet);

        assert!(deserialized_packet.is_ok());
    }

    #[test]
    fn compute_unit_limit_above_static_builtins() {
        // Cases:
        // 1. compute_unit_limit under static builtins
        // 2. compute_unit_limit equal to static builtins
        // 3. compute_unit_limit above static builtins
        for (cu_limit, expectation) in [(250, false), (300, true), (350, true)] {
            let keypair = Keypair::new();
            let bpf_program_id = Pubkey::new_unique();
            let ixs = vec![
                system_instruction::transfer(&keypair.pubkey(), &Pubkey::new_unique(), 1),
                compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
                Instruction::new_with_bytes(bpf_program_id, &[], vec![]), // non-builtin - not counted in filter
            ];
            let tx = Transaction::new_signed_with_payer(
                &ixs,
                Some(&keypair.pubkey()),
                &[&keypair],
                Hash::new_unique(),
            );
            let packet = Packet::from_data(None, tx).unwrap();
            let deserialized_packet = ImmutableDeserializedPacket::new(packet).unwrap();
            assert_eq!(
                deserialized_packet.compute_unit_limit_above_static_builtins(),
                expectation
            );
        }
    }
}
