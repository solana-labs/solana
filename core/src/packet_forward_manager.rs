use {
    crate::{
        banking_stage::TOTAL_BUFFERED_PACKETS, qos_service::QosService,
        unprocessed_packet_batches::*,
    },
    solana_perf::packet::Packet,
    solana_runtime::{bank::Bank, cost_model::TransactionCost, cost_tracker::CostTracker},
    solana_sdk::transaction::SanitizedTransaction,
    std::sync::Arc,
};

#[derive(Debug)]
pub struct PacketForwardManager<'a> {
    cost_trackers: Vec<CostTracker>,
    forwardable_packets: Vec<Vec<&'a Packet>>,
}

impl<'a> PacketForwardManager<'a> {
    pub fn new(max_forward_block_count: u64) -> Self {
        Self {
            cost_trackers: (0..max_forward_block_count)
                .into_iter()
                .map(|_| CostTracker::default())
                .collect(),
            forwardable_packets: (0..max_forward_block_count)
                .into_iter()
                .map(|_| Vec::<&'a Packet>::new())
                .collect(),
        }
    }

    pub fn take(
        &mut self,
        unprocessed_packet_batches: &'a UnprocessedPacketBatches,
        working_bank: Arc<Bank>,
        qos_service: &'a QosService,
    ) -> Vec<&'a Packet> {
        let prioritized_forwardable_packet_locators = Self::prioritize_unforwarded_packets_by_fee(
            unprocessed_packet_batches,
            Some(working_bank.clone()),
        );

        // if we have a bank to work with, then sort packets by write-account buckets
        let (transactions, sanitized_locators) = sanitize_transactions(
            unprocessed_packet_batches,
            &prioritized_forwardable_packet_locators,
            &working_bank.feature_set,
            working_bank.vote_only_bank(),
            working_bank.as_ref(),
        );
        let transactions_costs = qos_service.compute_transaction_costs(transactions.iter());

        transactions
            .iter()
            .zip(transactions_costs.iter())
            .zip(sanitized_locators.iter())
            .for_each(|((tx, cost), packet_locator)| {
                if let Some(packet) =
                    Self::locate_packet(unprocessed_packet_batches, packet_locator)
                {
                    self.sort_into_buckets(tx, cost, packet)
                }
            });

        let mut result = Vec::<&'a Packet>::new();
        self.forwardable_packets
            .iter()
            .for_each(|v| result.extend(v));
        result
    }

    // prioritize unforwarded, unprocessed packets in buffered packet_batches by its fee/CU
    fn prioritize_unforwarded_packets_by_fee(
        unprocessed_packet_batches: &'a UnprocessedPacketBatches,
        working_bank: Option<Arc<Bank>>,
    ) -> Vec<PacketLocator> {
        let mut locators = Vec::<PacketLocator>::with_capacity(TOTAL_BUFFERED_PACKETS);
        unprocessed_packet_batches.iter().enumerate().for_each(
            |(batch_index, deserialized_packet_batch)| {
                if !deserialized_packet_batch.forwarded {
                    deserialized_packet_batch
                        .unprocessed_packets
                        .keys()
                        .for_each(|packet_index| {
                            locators.push(PacketLocator {
                                batch_index,
                                packet_index: *packet_index,
                            });
                        })
                }
            },
        );

        prioritize_by_fee(unprocessed_packet_batches, &locators, working_bank)
    }

    fn sort_into_buckets(
        &mut self,
        transaction: &SanitizedTransaction,
        cost: &TransactionCost,
        packet: &'a Packet,
    ) {
        // try to sort the `transaction` into one of outbound (virtual) blocks
        for (cost_tracker, forwardable_packets) in self
            .cost_trackers
            .iter_mut()
            .zip(self.forwardable_packets.iter_mut())
        {
            match cost_tracker.try_add(transaction, cost) {
                Ok(_) => {
                    forwardable_packets.push(packet);
                    return;
                }
                Err(_) => {}
            }
        }
    }

    fn locate_packet(
        unprocessed_packet_batches: &'a UnprocessedPacketBatches,
        locator: &PacketLocator,
    ) -> Option<&'a Packet> {
        let deserialized_packet_batch = unprocessed_packet_batches.get(locator.batch_index)?;
        deserialized_packet_batch.get_packet(locator.packet_index)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_perf::packet::{Packet, PacketBatch},
        solana_runtime::{
            bank::goto_end_of_slot,
            block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS,
            genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
        },
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            hash::Hash,
            message::Message,
            signature::{Keypair, Signer},
            system_instruction::{self},
            system_transaction,
            transaction::Transaction,
        },
        std::net::SocketAddr,
    };

    #[test]
    fn test_prioritize_unforwarded_packets_by_fee() {
        solana_logger::setup();
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1_000_000, &leader, 3);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 1000;
        genesis_config.fee_rate_governor.target_signatures_per_slot = 1;

        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);

        let key0 = Keypair::new();
        let key1 = Keypair::new();
        let ix0 = system_instruction::transfer(&key0.pubkey(), &key1.pubkey(), 1);
        let ix1 = system_instruction::transfer(&key1.pubkey(), &key0.pubkey(), 1);
        let ix_cb = ComputeBudgetInstruction::request_units(1000, 20000);

        // build a buffer of 3 packet batches, 2nd bacth is forwarded
        let unprocessed_packets = vec![
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![Packet::from_data(
                    None,
                    &Transaction::new(
                        &[&key0],
                        Message::new(&[ix0.clone()], Some(&key0.pubkey())),
                        bank.last_blockhash(),
                    ),
                )
                .unwrap()]),
                vec![0],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![Packet::from_data(
                    None,
                    &Transaction::new(
                        &[&key1],
                        Message::new(&[ix1.clone()], Some(&key1.pubkey())),
                        bank.last_blockhash(),
                    ),
                )
                .unwrap()]),
                vec![0],
                true, // forwarded
            ),
            // add a packet with 2 signatures to buffer that has doubled fee, and additional fee
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![Packet::from_data(
                    None,
                    &Transaction::new(
                        &[&key0, &key1],
                        Message::new(&[ix0, ix1, ix_cb], Some(&key0.pubkey())),
                        bank.last_blockhash(),
                    ),
                )
                .unwrap()]),
                vec![0],
                false,
            ),
        ]
        .into_iter()
        .collect();

        {
            // If no bank is given, fee-per-cu won't calculate, should expect output is same as input
            let locators = vec![
                PacketLocator {
                    batch_index: 0,
                    packet_index: 0,
                },
                PacketLocator {
                    batch_index: 2,
                    packet_index: 0,
                },
            ];
            let prioritized_locators = PacketForwardManager::prioritize_unforwarded_packets_by_fee(
                &unprocessed_packets,
                None,
            );
            assert_eq!(locators, prioritized_locators);
        }

        {
            // If bank is given, fee-per-cu is calculated, should expect higher fee-per-cu come
            // out first
            let expected_locators = vec![
                PacketLocator {
                    batch_index: 2,
                    packet_index: 0,
                },
                PacketLocator {
                    batch_index: 0,
                    packet_index: 0,
                },
            ];

            let prioritized_locators = PacketForwardManager::prioritize_unforwarded_packets_by_fee(
                &unprocessed_packets,
                Some(Arc::new(bank)),
            );
            assert_eq!(expected_locators, prioritized_locators);
        }
    }

    #[test]
    fn test_locate_packet() {
        solana_logger::setup();
        let leader = solana_sdk::pubkey::new_rand();
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(1_000_000, &leader, 3);
        genesis_config
            .fee_rate_governor
            .target_lamports_per_signature = 1000;
        genesis_config.fee_rate_governor.target_signatures_per_slot = 1;

        let mut bank = Bank::new_for_tests(&genesis_config);
        let mut bank = Bank::new_from_parent(&Arc::new(bank), &leader, 1);
        goto_end_of_slot(&mut bank);

        let key0 = Keypair::new();
        let key1 = Keypair::new();
        let ix0 = system_instruction::transfer(&key0.pubkey(), &key1.pubkey(), 1);
        let ix1 = system_instruction::transfer(&key1.pubkey(), &key0.pubkey(), 1);
        let ix_cb = ComputeBudgetInstruction::request_units(1000, 20000);

        // build a buffer of 2 packet batches, 2nd bacth is forwarded
        let unprocessed_packets = vec![
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![Packet::from_data(
                    None,
                    &Transaction::new(
                        &[&key0],
                        Message::new(&[ix0.clone()], Some(&key0.pubkey())),
                        bank.last_blockhash(),
                    ),
                )
                .unwrap()]),
                vec![0],
                false,
            ),
            DeserializedPacketBatch::new(
                PacketBatch::new(vec![Packet::from_data(
                    None,
                    &Transaction::new(
                        &[&key1],
                        Message::new(&[ix1.clone()], Some(&key1.pubkey())),
                        bank.last_blockhash(),
                    ),
                )
                .unwrap()]),
                vec![0],
                true, // forwarded
            ),
        ]
        .into_iter()
        .collect();

        // locate packet succeffully
        assert!(PacketForwardManager::locate_packet(
            &unprocessed_packets,
            &PacketLocator {
                batch_index: 1,
                packet_index: 0
            }
        )
        .is_some());
        // batch_index out of bound
        assert!(PacketForwardManager::locate_packet(
            &unprocessed_packets,
            &PacketLocator {
                batch_index: 9,
                packet_index: 0
            }
        )
        .is_none());
        // packet_index out of bound
        assert!(PacketForwardManager::locate_packet(
            &unprocessed_packets,
            &PacketLocator {
                batch_index: 0,
                packet_index: 9
            }
        )
        .is_none());
    }

    #[test]
    fn test_sort_into_buckets() {
        solana_logger::setup();
        // a dummy transaction, it doesn't really matter.
        let dummy_tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let dummy_tx = SanitizedTransaction::from_transaction_for_tests(dummy_tx);
        // 2 writable accounts
        let account_a = solana_sdk::pubkey::new_rand();
        let account_b = solana_sdk::pubkey::new_rand();
        //
        let tx_cost_large_a = TransactionCost {
            writable_accounts: vec![account_a.clone()],
            execution_cost: MAX_WRITABLE_ACCOUNT_UNITS,
            ..TransactionCost::default()
        };
        let tx_cost_large_ab = TransactionCost {
            writable_accounts: vec![account_a.clone(), account_b.clone()],
            execution_cost: MAX_WRITABLE_ACCOUNT_UNITS,
            ..TransactionCost::default()
        };
        let tx_cost_small_a = TransactionCost {
            writable_accounts: vec![account_a.clone()],
            execution_cost: 1,
            ..TransactionCost::default()
        };
        let tx_cost_small_b = TransactionCost {
            writable_accounts: vec![account_b.clone()],
            execution_cost: 1,
            ..TransactionCost::default()
        };
        // packets
        let packet_1 =
            Packet::from_data(Some(&SocketAddr::from(([10, 10, 10, 1], 9001))), &42).unwrap();
        let packet_2 =
            Packet::from_data(Some(&SocketAddr::from(([10, 10, 10, 1], 9002))), &42).unwrap();
        let packet_3 =
            Packet::from_data(Some(&SocketAddr::from(([10, 10, 10, 1], 9003))), &42).unwrap();
        let packet_4 =
            Packet::from_data(Some(&SocketAddr::from(([10, 10, 10, 1], 9004))), &42).unwrap();
        let packet_5 =
            Packet::from_data(Some(&SocketAddr::from(([10, 10, 10, 1], 9005))), &42).unwrap();

        // setup 2 virtual blocks, which will have two account bucket: A and B
        // fee-sorted packets are:
        // (large, write A, packet_1)   - will fill up A in first block
        // (small, write B, packet_2)   - will put into B in first block
        // (large, write AB, packet_3)  - no room for it in first block's A nor B, will fill up A
        //                                in 2nd block
        // (small, write B, packet_4)   - will put into B in first block
        // (small, write A, packet_5)   - both As are filled up, this one will not be placed
        // where 'large' is account max limit cu, 'small' is just 1 cu
        // Expected result:
        // block 1
        //     A: packet_1
        //     B: packet_2, packet_4
        // block 2
        //     A: packet_3
        //     B: nil
        let mut forwarder = PacketForwardManager::new(2);
        forwarder.sort_into_buckets(&dummy_tx, &tx_cost_large_a, &packet_1);
        forwarder.sort_into_buckets(&dummy_tx, &tx_cost_small_b, &packet_2);
        forwarder.sort_into_buckets(&dummy_tx, &tx_cost_large_ab, &packet_3);
        forwarder.sort_into_buckets(&dummy_tx, &tx_cost_small_b, &packet_4);
        forwarder.sort_into_buckets(&dummy_tx, &tx_cost_small_a, &packet_5);

        assert_eq!(3, forwarder.forwardable_packets[0].len());
        assert_eq!(1, forwarder.forwardable_packets[1].len());

        assert_eq!(&packet_1, forwarder.forwardable_packets[0][0]);
        assert_eq!(&packet_2, forwarder.forwardable_packets[0][1]);
        assert_eq!(&packet_4, forwarder.forwardable_packets[0][2]);
        assert_eq!(&packet_3, forwarder.forwardable_packets[1][0]);

        let mut result = Vec::<&Packet>::new();
        forwarder
            .forwardable_packets
            .iter()
            .for_each(|v| result.extend(v));
        assert_eq!(4, result.len());
        assert_eq!(&packet_1, result[0]);
        assert_eq!(&packet_2, result[1]);
        assert_eq!(&packet_4, result[2]);
        assert_eq!(&packet_3, result[3]);
    }
}
