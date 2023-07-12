use {
    super::*,
    solana_entry::entry::Entry,
    solana_ledger::shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
    solana_sdk::{hash::Hash, signature::Keypair},
};

#[derive(Clone)]
pub(super) struct BroadcastFakeShredsRun {
    last_blockhash: Hash,
    partition: usize,
    shred_version: u16,
    next_code_index: u32,
    reed_solomon_cache: Arc<ReedSolomonCache>,
}

impl BroadcastFakeShredsRun {
    pub(super) fn new(partition: usize, shred_version: u16) -> Self {
        Self {
            last_blockhash: Hash::default(),
            partition,
            shred_version,
            next_code_index: 0,
            reed_solomon_cache: Arc::<ReedSolomonCache>::default(),
        }
    }
}

impl BroadcastRun for BroadcastFakeShredsRun {
    fn run(
        &mut self,
        keypair: &Keypair,
        blockstore: &Blockstore,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let bank = receive_results.bank;
        let last_tick_height = receive_results.last_tick_height;

        let next_shred_index = blockstore
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0) as u32;

        let num_entries = receive_results.entries.len();

        let shredder = Shredder::new(
            bank.slot(),
            bank.parent().unwrap().slot(),
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
            self.shred_version,
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
            keypair,
            &receive_results.entries,
            last_tick_height == bank.max_tick_height(),
            next_shred_index,
            self.next_code_index,
            true, // merkle_variant
            &self.reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );

        // If the last blockhash is default, a new block is being created
        // So grab the last blockhash from the parent bank
        if self.last_blockhash == Hash::default() {
            self.last_blockhash = bank.parent().unwrap().last_blockhash();
        }

        let fake_entries: Vec<_> = (0..num_entries)
            .map(|_| Entry::new(&self.last_blockhash, 0, vec![]))
            .collect();

        let (fake_data_shreds, fake_coding_shreds) = shredder.entries_to_shreds(
            keypair,
            &fake_entries,
            last_tick_height == bank.max_tick_height(),
            next_shred_index,
            self.next_code_index,
            true, // merkle_variant
            &self.reed_solomon_cache,
            &mut ProcessShredsStats::default(),
        );

        if let Some(index) = coding_shreds
            .iter()
            .chain(&fake_coding_shreds)
            .map(Shred::index)
            .max()
        {
            self.next_code_index = index + 1;
        }

        // If it's the last tick, reset the last block hash to default
        // this will cause next run to grab last bank's blockhash
        if last_tick_height == bank.max_tick_height() {
            self.last_blockhash = Hash::default();
        }

        let data_shreds = Arc::new(data_shreds);
        blockstore_sender.send((data_shreds.clone(), None))?;

        let slot = bank.slot();
        let batch_info = BroadcastShredBatchInfo {
            slot,
            num_expected_batches: None,
            slot_start_ts: Instant::now(),
            was_interrupted: false,
        };
        // 3) Start broadcast step
        //some indicates fake shreds
        let batch_info = Some(batch_info);
        assert!(fake_data_shreds.iter().all(|shred| shred.slot() == slot));
        assert!(fake_coding_shreds.iter().all(|shred| shred.slot() == slot));
        socket_sender.send((Arc::new(fake_data_shreds), batch_info.clone()))?;
        socket_sender.send((Arc::new(fake_coding_shreds), batch_info))?;
        //none indicates real shreds
        socket_sender.send((data_shreds, None))?;
        socket_sender.send((Arc::new(coding_shreds), None))?;

        Ok(())
    }
    fn transmit(
        &mut self,
        receiver: &TransmitReceiver,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
        _bank_forks: &RwLock<BankForks>,
        _quic_endpoint_sender: &AsyncSender<(SocketAddr, Bytes)>,
    ) -> Result<()> {
        for (data_shreds, batch_info) in receiver {
            let fake = batch_info.is_some();
            let peers = cluster_info.tvu_peers();
            peers.iter().enumerate().for_each(|(i, peer)| {
                if fake == (i <= self.partition) {
                    // Send fake shreds to the first N peers
                    if let Ok(addr) = peer.tvu(Protocol::UDP) {
                        data_shreds.iter().for_each(|b| {
                            sock.send_to(b.payload(), addr).unwrap();
                        });
                    }
                }
            });
        }
        Ok(())
    }
    fn record(&mut self, receiver: &RecordReceiver, blockstore: &Blockstore) -> Result<()> {
        for (data_shreds, _) in receiver {
            blockstore.insert_shreds(data_shreds.to_vec(), None, true)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_gossip::contact_info::ContactInfo,
        solana_sdk::signature::Signer,
        solana_streamer::socket::SocketAddrSpace,
        std::net::{IpAddr, Ipv4Addr, SocketAddr},
    };

    #[test]
    fn test_tvu_peers_ordering() {
        let cluster = {
            let keypair = Arc::new(Keypair::new());
            let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
            ClusterInfo::new(contact_info, keypair, SocketAddrSpace::Unspecified)
        };
        for k in 1..5 {
            cluster.insert_info(ContactInfo::new_with_socketaddr(
                &Keypair::new().pubkey(),
                &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, k)), 8080),
            ));
        }
        let tvu_peers1 = cluster.tvu_peers();
        (0..5).for_each(|_| {
            cluster
                .tvu_peers()
                .iter()
                .zip(tvu_peers1.iter())
                .for_each(|(v1, v2)| {
                    assert_eq!(v1, v2);
                });
        });
    }
}
