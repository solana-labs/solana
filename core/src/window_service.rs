//! `window_service` handles the data plane incoming blobs, storing them in
//!   blocktree and retransmitting where required
//!
use crate::cluster_info::ClusterInfo;
use crate::repair_service::{RepairService, RepairStrategy};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::{PacketReceiver, PacketSender};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use rayon::ThreadPool;
use solana_ledger::blocktree::{self, Blocktree};
use solana_ledger::leader_schedule_cache::LeaderScheduleCache;
use solana_ledger::shred::Shred;
use solana_metrics::{inc_new_counter_debug, inc_new_counter_error};
use solana_rayon_threadlimit::get_thread_count;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

fn verify_shred_slot(shred: &Shred, root: u64) -> bool {
    if shred.is_data() {
        // Only data shreds have parent information
        blocktree::verify_shred_slots(shred.slot(), shred.parent(), root)
    } else {
        // Filter out outdated coding shreds
        shred.slot() >= root
    }
}

/// drop blobs that are from myself or not from the correct leader for the
/// blob's slot
pub fn should_retransmit_and_persist(
    shred: &Shred,
    bank: Option<Arc<Bank>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    my_pubkey: &Pubkey,
    root: u64,
) -> bool {
    let slot_leader_pubkey = match bank {
        None => leader_schedule_cache.slot_leader_at(shred.slot(), None),
        Some(bank) => leader_schedule_cache.slot_leader_at(shred.slot(), Some(&bank)),
    };
    if let Some(leader_id) = slot_leader_pubkey {
        if leader_id == *my_pubkey {
            inc_new_counter_debug!("streamer-recv_window-circular_transmission", 1);
            false
        } else if !verify_shred_slot(shred, root) {
            inc_new_counter_debug!("streamer-recv_window-outdated_transmission", 1);
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
    thread_pool: &ThreadPool,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
) -> Result<()>
where
    F: Fn(&Shred, u64) -> bool + Sync,
{
    let timer = Duration::from_millis(200);
    let mut packets = r.recv_timeout(timer)?;

    while let Ok(mut more_packets) = r.try_recv() {
        packets.packets.append(&mut more_packets.packets)
    }
    let now = Instant::now();
    inc_new_counter_debug!("streamer-recv_window-recv", packets.packets.len());

    let last_root = blocktree.last_root();
    let shreds: Vec<_> = thread_pool.install(|| {
        packets
            .packets
            .par_iter_mut()
            .filter_map(|packet| {
                if let Ok(shred) = Shred::new_from_serialized_shred(packet.data.to_vec()) {
                    if shred_filter(&shred, last_root) {
                        packet.meta.slot = shred.slot();
                        packet.meta.seed = shred.seed();
                        Some(shred)
                    } else {
                        packet.meta.discard = true;
                        None
                    }
                } else {
                    packet.meta.discard = true;
                    None
                }
            })
            .collect()
    });

    trace!("{:?} shreds from packets", shreds.len());

    trace!(
        "{} num shreds received: {}",
        my_pubkey,
        packets.packets.len()
    );

    if !packets.packets.is_empty() {
        // Ignore the send error, as the retransmit is optional (e.g. archivers don't retransmit)
        let _ = retransmit.send(packets);
    }

    blocktree.insert_shreds(shreds, Some(leader_schedule_cache))?;

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
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        shred_filter: F,
    ) -> WindowService
    where
        F: 'static
            + Fn(&Pubkey, &Shred, Option<Arc<Bank>>, u64) -> bool
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
        let leader_schedule_cache = leader_schedule_cache.clone();
        let t_window = Builder::new()
            .name("solana-window".to_string())
            // TODO: Mark: Why is it overflowing
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let _exit = Finalizer::new(exit.clone());
                let id = cluster_info.read().unwrap().id();
                trace!("{}: RECV_WINDOW started", id);
                let mut now = Instant::now();
                let thread_pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .build()
                    .unwrap();
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(e) = recv_window(
                        &blocktree,
                        &id,
                        &r,
                        &retransmit,
                        |shred, last_root| {
                            shred_filter(
                                &id,
                                shred,
                                bank_forks
                                    .as_ref()
                                    .map(|bank_forks| bank_forks.read().unwrap().working_bank()),
                                last_root,
                            )
                        },
                        &thread_pool,
                        &leader_schedule_cache,
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
    use crate::{
        cluster_info::ClusterInfo,
        contact_info::ContactInfo,
        genesis_utils::create_genesis_block_with_leader,
        packet::{Packet, Packets},
        repair_service::RepairSlotRange,
        service::Service,
    };
    use rand::{seq::SliceRandom, thread_rng};
    use solana_ledger::shred::DataShredHeader;
    use solana_ledger::{
        blocktree::{get_tmp_ledger_path, make_many_slot_entries, Blocktree},
        entry::{create_ticks, Entry},
        shred::Shredder,
    };
    use solana_sdk::{
        epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
        hash::Hash,
        signature::{Keypair, KeypairUtil},
    };
    use std::{
        net::UdpSocket,
        sync::atomic::{AtomicBool, Ordering},
        sync::mpsc::{channel, Receiver},
        sync::{Arc, RwLock},
        thread::sleep,
        time::Duration,
    };

    fn local_entries_to_shred(
        entries: &[Entry],
        slot: u64,
        parent: u64,
        keypair: &Arc<Keypair>,
    ) -> Vec<Shred> {
        let shredder = Shredder::new(slot, parent, 0.0, keypair.clone())
            .expect("Failed to create entry shredder");
        shredder.entries_to_shreds(&entries, true, 0).0
    }

    #[test]
    fn test_process_shred() {
        let blocktree_path = get_tmp_ledger_path!();
        let blocktree = Arc::new(Blocktree::open(&blocktree_path).unwrap());
        let num_entries = 10;
        let original_entries = create_ticks(num_entries, Hash::default());
        let mut shreds = local_entries_to_shred(&original_entries, 0, 0, &Arc::new(Keypair::new()));
        shreds.reverse();
        blocktree
            .insert_shreds(shreds, None)
            .expect("Expect successful processing of shred");

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
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let bank = Arc::new(Bank::new(
            &create_genesis_block_with_leader(100, &leader_pubkey, 10).genesis_block,
        ));
        let cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

        let mut shreds = local_entries_to_shred(&[Entry::default()], 0, 0, &leader_keypair);

        // with a Bank for slot 0, blob continues
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id, 0,),
            true
        );

        // If it's a coding shred, test that slot >= root
        let (common, coding) = Shredder::new_coding_shred_header(5, 5, 6, 6, 0);
        let mut coding_shred =
            Shred::new_empty_from_header(common, DataShredHeader::default(), coding);
        Shredder::sign_shred(&leader_keypair, &mut coding_shred);
        assert_eq!(
            should_retransmit_and_persist(&coding_shred, Some(bank.clone()), &cache, &me_id, 0),
            true
        );
        assert_eq!(
            should_retransmit_and_persist(&coding_shred, Some(bank.clone()), &cache, &me_id, 5),
            true
        );
        assert_eq!(
            should_retransmit_and_persist(&coding_shred, Some(bank.clone()), &cache, &me_id, 6),
            false
        );

        // set the blob to have come from the wrong leader
        let wrong_leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = wrong_leader_keypair.pubkey();
        let wrong_bank = Arc::new(Bank::new(
            &create_genesis_block_with_leader(100, &leader_pubkey, 10).genesis_block,
        ));
        let wrong_cache = Arc::new(LeaderScheduleCache::new_from_bank(&wrong_bank));
        assert_eq!(
            should_retransmit_and_persist(
                &shreds[0],
                Some(wrong_bank.clone()),
                &wrong_cache,
                &me_id,
                0
            ),
            false
        );

        // with a Bank and no idea who leader is, blob gets thrown out
        shreds[0].set_slot(MINIMUM_SLOTS_PER_EPOCH as u64 * 3);
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id, 0),
            false
        );

        // with a shred where shred.slot() == root, blob gets thrown out
        let slot = MINIMUM_SLOTS_PER_EPOCH as u64 * 3;
        let shreds = local_entries_to_shred(&[Entry::default()], slot, slot - 1, &leader_keypair);
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id, slot),
            false
        );

        // with a shred where shred.parent() < root, blob gets thrown out
        let slot = MINIMUM_SLOTS_PER_EPOCH as u64 * 3;
        let shreds =
            local_entries_to_shred(&[Entry::default()], slot + 1, slot - 1, &leader_keypair);
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], Some(bank.clone()), &cache, &me_id, slot),
            false
        );

        // if the blob came back from me, it doesn't continue, whether or not I have a bank
        assert_eq!(
            should_retransmit_and_persist(&shreds[0], None, &cache, &me_id, 0),
            false
        );
    }

    fn make_test_window(
        packet_receiver: Receiver<Packets>,
        exit: Arc<AtomicBool>,
    ) -> WindowService {
        let blocktree_path = get_tmp_ledger_path!();
        let (blocktree, _, _) = Blocktree::open_with_signal(&blocktree_path)
            .expect("Expected to be able to open database ledger");

        let blocktree = Arc::new(blocktree);
        let (retransmit_sender, _retransmit_receiver) = channel();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::new_localhost(&Pubkey::default(), 0),
        )));
        let repair_sock = Arc::new(UdpSocket::bind(socketaddr_any!()).unwrap());
        let window = WindowService::new(
            blocktree,
            cluster_info,
            packet_receiver,
            retransmit_sender,
            repair_sock,
            &exit,
            RepairStrategy::RepairRange(RepairSlotRange { start: 0, end: 0 }),
            &Arc::new(LeaderScheduleCache::default()),
            |_, _, _, _| true,
        );
        window
    }

    #[test]
    fn test_recv_window() {
        let (packet_sender, packet_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let window = make_test_window(packet_receiver, exit.clone());
        // send 5 slots worth of data to the window
        let (shreds, _) = make_many_slot_entries(0, 5, 10);
        let packets: Vec<_> = shreds
            .into_iter()
            .map(|mut s| {
                let mut p = Packet::default();
                p.data.copy_from_slice(&mut s.payload);
                p
            })
            .collect();
        let mut packets = Packets::new(packets);
        packet_sender.send(packets.clone()).unwrap();
        sleep(Duration::from_millis(500));

        // add some empty packets to the data set. These should fail to deserialize
        packets.packets.append(&mut vec![Packet::default(); 10]);
        packets.packets.shuffle(&mut thread_rng());
        packet_sender.send(packets.clone()).unwrap();
        sleep(Duration::from_millis(500));

        // send 1 empty packet that cannot deserialize into a shred
        packet_sender
            .send(Packets::new(vec![Packet::default(); 1]))
            .unwrap();
        sleep(Duration::from_millis(500));

        exit.store(true, Ordering::Relaxed);
        window.join().unwrap();
    }
}
