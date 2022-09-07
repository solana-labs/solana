use {
    solana_perf::packet::Packet,
    solana_runtime::transaction_priority_details::{
        GetTransactionPriorityDetails, TransactionPriorityDetails,
    },
    solana_sdk::{
        feature_set,
        hash::Hash,
        message::Message,
        sanitize::SanitizeError,
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
    pub fn new(
        packet: Packet,
        priority_details: Option<TransactionPriorityDetails>,
    ) -> Result<Self, DeserializedPacketError> {
        let versioned_transaction: VersionedTransaction = packet.deserialize_slice(..)?;
        let sanitized_transaction = SanitizedVersionedTransaction::try_from(versioned_transaction)?;
        let message_bytes = packet_message(&packet)?;
        let message_hash = Message::hash_raw_message(message_bytes);
        let is_simple_vote = packet.meta.is_simple_vote_tx();

        // drop transaction if prioritization fails.
        let priority_details = priority_details
            .or_else(|| sanitized_transaction.get_transaction_priority_details())
            .ok_or(DeserializedPacketError::PrioritizationFailure)?;

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

// This function deserializes packets into transactions, computes the blake3 hash of transaction
// messages, and verifies secp256k1 instructions. A list of sanitized transactions are returned
// with their packet indexes.
pub fn transaction_from_deserialized_packet(
    deserialized_packet: &ImmutableDeserializedPacket,
    feature_set: &Arc<feature_set::FeatureSet>,
    votes_only: bool,
    address_loader: impl AddressLoader,
) -> Option<SanitizedTransaction> {
    if votes_only && !deserialized_packet.is_simple_vote() {
        return None;
    }

    let tx = SanitizedTransaction::try_new(
        deserialized_packet.transaction().clone(),
        *deserialized_packet.message_hash(),
        deserialized_packet.is_simple_vote(),
        address_loader,
    )
    .ok()?;
    tx.verify_precompiles(feature_set).ok()?;
    Some(tx)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_perf::packet::PacketFlags,
        solana_sdk::{
            feature_set::FeatureSet,
            signature::Keypair,
            signer::Signer,
            system_transaction,
            transaction::{SimpleAddressLoader, Transaction},
        },
        solana_vote_program::vote_transaction,
        std::sync::Arc,
    };

    #[test]
    fn simple_deserialized_packet() {
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, &tx).unwrap();
        let deserialized_packet = ImmutableDeserializedPacket::new(packet, None);

        assert!(matches!(deserialized_packet, Ok(_)));
    }

    #[cfg(test)]
    fn make_test_packets(
        transactions: Vec<Transaction>,
        vote_indexes: Vec<usize>,
    ) -> Vec<ImmutableDeserializedPacket> {
        let capacity = transactions.len();
        let mut packet_vector = Vec::with_capacity(capacity);
        for tx in transactions.iter() {
            packet_vector.push(Packet::from_data(None, &tx).unwrap());
        }
        for index in vote_indexes.iter() {
            packet_vector[*index].meta.flags |= PacketFlags::SIMPLE_VOTE_TX;
        }

        packet_vector
            .into_iter()
            .map(|p| ImmutableDeserializedPacket::new(p, None).unwrap())
            .collect()
    }

    #[test]
    fn test_transaction_from_deserialized_packet() {
        let keypair = Keypair::new();
        let transfer_tx =
            system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default());
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![42],
            Hash::default(),
            Hash::default(),
            &keypair,
            &keypair,
            &keypair,
            None,
        );

        // packets with no votes
        {
            let vote_indexes = vec![];
            let packet_vector =
                make_test_packets(vec![transfer_tx.clone(), transfer_tx.clone()], vote_indexes);

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(2, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(0, txs.count());
        }

        // packets with some votes
        {
            let vote_indexes = vec![0, 2];
            let packet_vector = make_test_packets(
                vec![vote_tx.clone(), transfer_tx, vote_tx.clone()],
                vote_indexes,
            );

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(2, txs.count());
        }

        // packets with all votes
        {
            let vote_indexes = vec![0, 1, 2];
            let packet_vector = make_test_packets(
                vec![vote_tx.clone(), vote_tx.clone(), vote_tx],
                vote_indexes,
            );

            let mut votes_only = false;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());

            votes_only = true;
            let txs = packet_vector.iter().filter_map(|tx| {
                transaction_from_deserialized_packet(
                    tx,
                    &Arc::new(FeatureSet::default()),
                    votes_only,
                    SimpleAddressLoader::Disabled,
                )
            });
            assert_eq!(3, txs.count());
        }
    }
}
