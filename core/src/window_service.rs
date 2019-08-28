//! `window_service` handles the data plane incoming blobs, storing them in
//!   blocktree and retransmitting where required
//!
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::repair_service::{RepairService, RepairStrategy};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::shred::Shred;
use crate::streamer::{PacketReceiver, PacketSender};
use solana_metrics::{inc_new_counter_debug, inc_new_counter_error};
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub const NUM_THREADS: u32 = 10;

/// Process a blob: Add blob to the ledger window.
pub fn process_shreds(shreds: Vec<Shred>, blocktree: &Arc<Blocktree>) -> Result<()> {
    blocktree.insert_shreds(shreds)
}

/// drop blobs that are from myself or not from the correct leader for the
/// blob's slot
pub fn should_retransmit_and_persist(
    shred: &Shred,
    bank: Option<Arc<Bank>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    my_pubkey: &Pubkey,
) -> bool {
    let slot_leader_pubkey = match bank {
        None => leader_schedule_cache.slot_leader_at(shred.slot(), None),
        Some(bank) => leader_schedule_cache.slot_leader_at(shred.slot(), Some(&bank)),
    };

    if let Some(leader_id) = slot_leader_pubkey {
        if leader_id == *my_pubkey {
            inc_new_counter_debug!("streamer-recv_window-circular_transmission", 1);
            false
        } else if !shred.verify(&leader_id) {
            inc_new_counter_debug!("streamer-recv_window-invalid_signature", 1);
            false
        } else {
            true
        }
    } else {
        inc_new_counter_debug!("streamer-recv_window-unknown_leader", 1);
        false
    }
}

fn recv_window<F>(
    blocktree: &Arc<Blocktree>,
    my_pubkey: &Pubkey,
    r: &PacketReceiver,
    retransmit: &PacketSender,
    shred_filter: F,
) -> Result<()>
where
    F: Fn(&Shred) -> bool,
    F: Sync,
{
    let timer = Duration::from_millis(200);
    let mut packets = r.recv_timeout(timer)?;

    while let Ok(mut more_packets) = r.try_recv() {
        packets.packets.append(&mut more_packets.packets)
    }
    let now = Instant::now();
    inc_new_counter_debug!("streamer-recv_window-recv", packets.packets.len());

    let mut shreds = vec![];
    let mut discards = vec![];
    for (i, packet) in packets.packets.iter_mut().enumerate() {
        if let Ok(s) = bincode::deserialize(&packet.data) {
            let shred: Shred = s;
            if shred_filter(&shred) {
                packet.meta.slot = shred.slot();
                packet.meta.seed = shred.seed();
                shreds.push(shred);
            } else {
                discards.push(i);
            }
        } else {
            discards.push(i);
        }
    }

    for i in discards.into_iter().rev() {
        packets.packets.remove(i);
    }

    trace!("{:?} shreds from packets", shreds.len());

    trace!(
        "{} num shreds received: {}",
        my_pubkey,
        packets.packets.len()
    );

    if !packets.packets.is_empty() {
        // Ignore the send error, as the retransmit is optional (e.g. replicators don't retransmit)
        let _ = retransmit.send(packets);
    }

    blocktree.insert_shreds(shreds)?;

    trace!(
        "Elapsed processing time in recv_window(): {}",
        duration_as_ms(&now.elapsed())
    );

    Ok(())
}

// Implement a destructor for the window_service thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct WindowService {
    t_window: JoinHandle<()>,
    repair_service: RepairService,
}

impl WindowService {
    #[allow(clippy::too_many_arguments)]
    pub fn new<F>(
        blocktree: Arc<Blocktree>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        r: PacketReceiver,
        retransmit: PacketSender,
        repair_socket: Arc<UdpSocket>,
        exit: &Arc<AtomicBool>,
        repair_strategy: RepairStrategy,
        shred_filter: F,
    ) -> WindowService
    where
        F: 'static
            + Fn(&Pubkey, &Shred, Option<Arc<Bank>>) -> bool
            + std::marker::Send
            + std::marker::Sync,
    {
        let bank_forks = match repair_strategy {
            RepairStrategy::RepairRange(_) => None,

            RepairStrategy::RepairAll { ref bank_forks, .. } => Some(bank_forks.clone()),
        };

        let repair_service = RepairService::new(
            blocktree.clone(),
            exit.clone(),
            repair_socket,
            cluster_info.clone(),
            repair_strategy,
        );
        let exit = exit.clone();
        let shred_filter = Arc::new(shred_filter);
        let bank_forks = bank_forks.clone();
        let t_window = Builder::new()
            .name("solana-window".to_string())
            // TODO: Mark: Why is it overflowing
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let _exit = Finalizer::new(exit.clone());
                let id = cluster_info.read().unwrap().id();
                trace!("{}: RECV_WINDOW started", id);
                let mut now = Instant::now();
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(e) = recv_window(
                        &blocktree,
                        &id,
                        &r,
                        &retransmit,
                        |shred| {
                            shred_filter(
                                &id,
                                shred,
                                bank_forks
                                    .as_ref()
                                    .map(|bank_forks| bank_forks.read().unwrap().working_bank()),
                            )
                        },
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => {
                                if now.elapsed() > Duration::from_secs(30) {
                                    warn!("Window does not seem to be receiving data. Ensure port configuration is correct...");
                                    now = Instant::now();
                                }
                            }
                            _ => {
                                inc_new_counter_error!("streamer-window-error", 1, 1);
                                error!("window error: {:?}", e);
                            }
                        }
                    } else {
                        now = Instant::now();
                    }
                }
            })
            .unwrap();

        WindowService {
            t_window,
            repair_service,
        }
    }
}

impl Service for WindowService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_window.join()?;
        self.repair_service.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank_forks::BankForks;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::{make_consecutive_blobs, make_tiny_test_entries, Entry};
    use crate::genesis_utils::create_genesis_block_with_leader;
    use crate::recycler::Recycler;
    use crate::service::Service;
    use crate::shred::Shredder;
    use crate::streamer::{receiver, responder};
    use solana_runtime::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn local_entries_to_shred(entries: Vec<Entry>, keypair: &Arc<Keypair>) -> Vec<Shred> {
        let mut shredder =
            Shredder::new(0, Some(0), 0.0, keypair, 0).expect("Failed to create entry shredder");
        bincode::serialize_into(&mut shredder, &entries)
            .expect("Expect to write all entries to shreds");
        shredder.finalize_slot();
        shredder
            .shreds
            .iter()
            .map(|s| bincode::deserialize(s).unwrap())
            .collect()
    }

    #[test]
    fn test_process_blob() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Arc::new(Blocktree::open(&blocktree_path).unwrap());
        let num_entries = 10;
        let original_entries = make_tiny_test_entries(num_entries);
        let shreds = local_entries_to_shred(original_entries.clone(), &Arc::new(Keypair::new()));

        for shred in shreds.into_iter().rev() {
            process_shreds(vec![shred], &blocktree).expect("Expect successful processing of blob");
        }

        assert_eq!(
            blocktree.get_slot_entries(0, 0, None).unwrap(),
            original_entries
        );

        drop(blocktree);
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_should_retransmit_and_persist() {
        let me_id = Pubkey::new_rand();
        let leader_keypair = Keypair::new();
        let leader_pubkey = leader_keypair.pubkey();
        let bank = Arc::new(Bank::new(
            &create_genesis_block_with_leader(100, &leader_pubkey, 10).genesis_block,
        ));
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

        let entry = Entry::default();
        let mut shreds = local_entries_to_shred(vec![entry], &Arc::new(leader_keypair));

        // with a Bank for slot 0, blob continues
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id),
            true
        );

        // set the blob to have come from the wrong leader
        /*
                assert_eq!(
                    should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id),
                    false
                );
        */

        // with a Bank and no idea who leader is, blob gets thrown out
        shreds[0].set_slot(MINIMUM_SLOTS_PER_EPOCH as u64 * 3);
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank), &cache, &me_id),
            false
        );

        // if the blob came back from me, it doesn't continue, whether or not I have a bank
        /*
                assert_eq!(
                    should_retransmit_and_persist(&shreds[0], None, &cache, &me_id),
                    false
                );
        */
    }

    #[test]
    #[ignore]
    pub fn window_send_test() {
        solana_logger::setup();
        // setup a leader whose id is used to generates blobs and a validator
        // node whose window service will retransmit leader blobs.
        let leader_node = Node::new_localhost();
        let validator_node = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let cluster_info_me = ClusterInfo::new_with_invalid_keypair(validator_node.info.clone());
        let me_id = leader_node.info.id;
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(
            Arc::new(leader_node.sockets.gossip),
            &exit,
            s_reader,
            Recycler::default(),
            "window_send_test",
        );
        let (s_retransmit, r_retransmit) = channel();
        let blocktree_path = get_tmp_ledger_path!();
        let (blocktree, _, completed_slots_receiver) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to be able to open database ledger");
        let blocktree = Arc::new(blocktree);

        let bank = Bank::new(&create_genesis_block_with_leader(100, &me_id, 10).genesis_block);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let repair_strategy = RepairStrategy::RepairAll {
            bank_forks: bank_forks.clone(),
            completed_slots_receiver,
            epoch_schedule: bank_forks
                .read()
                .unwrap()
                .working_bank()
                .epoch_schedule()
                .clone(),
        };
        let t_window = WindowService::new(
            blocktree,
            subs,
            r_reader,
            s_retransmit,
            Arc::new(leader_node.sockets.repair),
            &exit,
            repair_strategy,
            |_, _, _| true,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                leader_node.sockets.tvu.into_iter().map(Arc::new).collect();

            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let num_blobs_to_make = 10;
            let gossip_address = &leader_node.info.gossip;
            let msgs = make_consecutive_blobs(
                &me_id,
                num_blobs_to_make,
                0,
                Hash::default(),
                &gossip_address,
            )
            .into_iter()
            .rev()
            .collect();
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let max_attempts = 10;
        let mut num_attempts = 0;
        let mut q = Vec::new();
        loop {
            assert!(num_attempts != max_attempts);
            while let Ok(mut nq) = r_retransmit.recv_timeout(Duration::from_millis(500)) {
                q.append(&mut nq.packets);
            }
            if q.len() == 10 {
                break;
            }
            num_attempts += 1;
        }

        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&blocktree_path);
    }

    #[test]
    #[ignore]
    pub fn window_send_leader_test2() {
        solana_logger::setup();
        // setup a leader whose id is used to generates blobs and a validator
        // node whose window service will retransmit leader blobs.
        let leader_node = Node::new_localhost();
        let validator_node = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let cluster_info_me = ClusterInfo::new_with_invalid_keypair(validator_node.info.clone());
        let me_id = leader_node.info.id;
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(
            Arc::new(leader_node.sockets.gossip),
            &exit,
            s_reader,
            Recycler::default(),
            "window_send_leader_test2",
        );
        let (s_retransmit, r_retransmit) = channel();
        let blocktree_path = get_tmp_ledger_path!();
        let (blocktree, _, completed_slots_receiver) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to be able to open database ledger");

        let blocktree = Arc::new(blocktree);
        let bank = Bank::new(&create_genesis_block_with_leader(100, &me_id, 10).genesis_block);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let epoch_schedule = *bank_forks.read().unwrap().working_bank().epoch_schedule();
        let repair_strategy = RepairStrategy::RepairAll {
            bank_forks,
            completed_slots_receiver,
            epoch_schedule,
        };
        let t_window = WindowService::new(
            blocktree,
            subs.clone(),
            r_reader,
            s_retransmit,
            Arc::new(leader_node.sockets.repair),
            &exit,
            repair_strategy,
            |_, _, _| true,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                leader_node.sockets.tvu.into_iter().map(Arc::new).collect();
            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let mut msgs = Vec::new();
            let blobs =
                make_consecutive_blobs(&me_id, 14u64, 0, Hash::default(), &leader_node.info.gossip);

            for v in 0..10 {
                let i = 9 - v;
                msgs.push(blobs[i].clone());
            }
            s_responder.send(msgs).expect("send");

            let mut msgs1 = Vec::new();
            for v in 1..5 {
                let i = 9 + v;
                msgs1.push(blobs[i].clone());
            }
            s_responder.send(msgs1).expect("send");
            t_responder
        };
        let mut q = Vec::new();
        while let Ok(mut nq) = r_retransmit.recv_timeout(Duration::from_millis(5000)) {
            q.append(&mut nq.packets);
        }
        assert!(q.len() > 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&blocktree_path);
    }
}
