use {
    super::packet_filter::PacketFilterFailure,
    solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
    solana_perf::packet::Packet,
    solana_runtime_transaction::instructions_processor::process_compute_budget_instructions,
    solana_sanitize::SanitizeError,
    solana_sdk::{
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::Signature,
        transaction::{
            AddressLoader, SanitizedTransaction, SanitizedVersionedTransaction,
            VersionedTransaction,
        },
    },
    solana_short_vec::decode_shortu16_len,
    solana_svm_transaction::instruction::SVMInstruction,
    std::{cmp::Ordering, collections::HashSet, mem::size_of},
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
    #[error("Packet filter failure: {0}")]
    FailedFilter(#[from] PacketFilterFailure),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImmutableDeserializedPacket {
    original_packet: Packet,
    transaction: SanitizedVersionedTransaction,
    message_hash: Hash,
    is_simple_vote: bool,
    compute_unit_price: u64,
    compute_unit_limit: u32,
}

impl ImmutableDeserializedPacket {
    pub fn new(packet: Packet) -> Result<Self, DeserializedPacketError> {
        let versioned_transaction: VersionedTransaction = packet.deserialize_slice(..)?;
        let sanitized_transaction = SanitizedVersionedTransaction::try_from(versioned_transaction)?;
        let message_bytes = packet_message(&packet)?;
        let message_hash = Message::hash_raw_message(message_bytes);
        let is_simple_vote = packet.meta().is_simple_vote_tx();

        // drop transaction if prioritization fails.
        let ComputeBudgetLimits {
            mut compute_unit_price,
            compute_unit_limit,
            ..
        } = process_compute_budget_instructions(
            sanitized_transaction
                .get_message()
                .program_instructions_iter()
                .map(|(pubkey, ix)| (pubkey, SVMInstruction::from(ix))),
        )
        .map_err(|_| DeserializedPacketError::PrioritizationFailure)?;

        // set compute unit price to zero for vote transactions
        if is_simple_vote {
            compute_unit_price = 0;
        };

        Ok(Self {
            original_packet: packet,
            transaction: sanitized_transaction,
            message_hash,
            is_simple_vote,
            compute_unit_price,
            compute_unit_limit,
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

    pub fn compute_unit_price(&self) -> u64 {
        self.compute_unit_price
    }

    pub fn compute_unit_limit(&self) -> u64 {
        u64::from(self.compute_unit_limit)
    }

    // This function deserializes packets into transactions, computes the blake3 hash of transaction
    // messages.
    pub fn build_sanitized_transaction(
        &self,
        votes_only: bool,
        address_loader: impl AddressLoader,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Option<SanitizedTransaction> {
        if votes_only && !self.is_simple_vote() {
            return None;
        }
        let tx = SanitizedTransaction::try_new(
            self.transaction().clone(),
            *self.message_hash(),
            self.is_simple_vote(),
            address_loader,
            reserved_account_keys,
        )
        .ok()?;
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
        self.compute_unit_price().cmp(&other.compute_unit_price())
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
        for (cu_limit, expectation) in [
            (250, Err(PacketFilterFailure::InsufficientComputeLimit)),
            (300, Ok(())),
            (350, Ok(())),
        ] {
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
                deserialized_packet.check_insufficent_compute_unit_limit(),
                expectation
            );
        }
    }
}
