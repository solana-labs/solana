use super::*;
use crate::entry::Entry;
use solana_sdk::hash::Hash;

pub(super) struct BroadcastFakeBlobsRun {
    last_blockhash: Hash,
    partition: usize,
}

impl BroadcastFakeBlobsRun {
    pub(super) fn new(partition: usize) -> Self {
        Self {
            last_blockhash: Hash::default(),
            partition,
        }
    }
}

impl BroadcastRun for BroadcastFakeBlobsRun {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntries>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let receive_results = broadcast_utils::recv_slot_shreds(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;

        let keypair = &cluster_info.read().unwrap().keypair.clone();
        let latest_blob_index = blocktree
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0);

        let (_, shred_bufs, _) = broadcast_utils::entries_to_shreds(
            receive_results.ventries,
            bank.slot(),
            receive_results.last_tick,
            bank.max_tick_height(),
            keypair,
            latest_blob_index,
            bank.parent().unwrap().slot(),
        );

        // If the last blockhash is default, a new block is being created
        // So grab the last blockhash from the parent bank
        if self.last_blockhash == Hash::default() {
            self.last_blockhash = bank.parent().unwrap().last_blockhash();
        }

        let fake_ventries: Vec<_> = (0..receive_results.num_entries)
            .map(|_| vec![(Entry::new(&self.last_blockhash, 0, vec![]), 0)])
            .collect();

        let (_fake_shreds, fake_shred_bufs, _) = broadcast_utils::entries_to_shreds(
            fake_ventries,
            bank.slot(),
            receive_results.last_tick,
            bank.max_tick_height(),
            keypair,
            latest_blob_index,
            bank.parent().unwrap().slot(),
        );

        // If it's the last tick, reset the last block hash to default
        // this will cause next run to grab last bank's blockhash
        if last_tick == bank.max_tick_height() {
            self.last_blockhash = Hash::default();
        }

        blocktree.insert_shreds(shred_bufs.clone(), None)?;
        // 3) Start broadcast step
        let peers = cluster_info.read().unwrap().tvu_peers();
        peers.iter().enumerate().for_each(|(i, peer)| {
            if i <= self.partition {
                // Send fake blobs to the first N peers
                fake_shred_bufs.iter().for_each(|b| {
                    sock.send_to(&b.shred, &peer.tvu_forwards).unwrap();
                });
            } else {
                shred_bufs.iter().for_each(|b| {
                    sock.send_to(&b.shred, &peer.tvu_forwards).unwrap();
                });
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_info::ContactInfo;
    use solana_sdk::pubkey::Pubkey;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_tvu_peers_ordering() {
        let mut cluster = ClusterInfo::new_with_invalid_keypair(ContactInfo::new_localhost(
            &Pubkey::new_rand(),
            0,
        ));
        cluster.insert_info(ContactInfo::new_with_socketaddr(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            8080,
        )));
        cluster.insert_info(ContactInfo::new_with_socketaddr(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            8080,
        )));
        cluster.insert_info(ContactInfo::new_with_socketaddr(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)),
            8080,
        )));
        cluster.insert_info(ContactInfo::new_with_socketaddr(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 4)),
            8080,
        )));

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
