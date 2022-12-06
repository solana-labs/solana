use {
    crossbeam_channel::{Receiver, RecvTimeoutError, SendError, Sender},
    solana_ledger::{
        leader_schedule_cache::LeaderScheduleCache, shred, sigverify_shreds::verify_shreds_gpu,
    },
    solana_perf::{self, packet::PacketBatch, recycler_cache::RecyclerCache},
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

#[allow(clippy::enum_variant_names)]
enum Error {
    RecvDisconnected,
    RecvTimeout,
    SendError,
}

pub(crate) fn spawn_shred_sigverify(
    // TODO: Hot swap will change pubkey.
    self_pubkey: Pubkey,
    bank_forks: Arc<RwLock<BankForks>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    shred_fetch_receiver: Receiver<PacketBatch>,
    retransmit_sender: Sender<Vec</*shred:*/ Vec<u8>>>,
    verified_sender: Sender<Vec<PacketBatch>>,
    turbine_disabled: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let recycler_cache = RecyclerCache::warmed();
    let mut stats = ShredSigVerifyStats::new(Instant::now());
    Builder::new()
        .name("solShredVerifr".to_string())
        .spawn(move || loop {
            match run_shred_sigverify(
                &self_pubkey,
                &bank_forks,
                &leader_schedule_cache,
                &recycler_cache,
                &shred_fetch_receiver,
                &retransmit_sender,
                &verified_sender,
                &turbine_disabled,
                &mut stats,
            ) {
                Ok(()) => (),
                Err(Error::RecvTimeout) => (),
                Err(Error::RecvDisconnected) => break,
                Err(Error::SendError) => break,
            }
            stats.maybe_submit();
        })
        .unwrap()
}

fn run_shred_sigverify(
    self_pubkey: &Pubkey,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    recycler_cache: &RecyclerCache,
    shred_fetch_receiver: &Receiver<PacketBatch>,
    retransmit_sender: &Sender<Vec</*shred:*/ Vec<u8>>>,
    verified_sender: &Sender<Vec<PacketBatch>>,
    turbine_disabled: &AtomicBool,
    stats: &mut ShredSigVerifyStats,
) -> Result<(), Error> {
    const RECV_TIMEOUT: Duration = Duration::from_secs(1);
    let packets = shred_fetch_receiver.recv_timeout(RECV_TIMEOUT)?;
    let mut packets: Vec<_> = std::iter::once(packets)
        .chain(shred_fetch_receiver.try_iter())
        .collect();
    let now = Instant::now();
    stats.num_iters += 1;
    stats.num_packets += packets.iter().map(PacketBatch::len).sum::<usize>();
    stats.num_discards_pre += count_discards(&packets);
    verify_packets(
        self_pubkey,
        bank_forks,
        leader_schedule_cache,
        recycler_cache,
        &mut packets,
    );
    stats.num_discards_post += count_discards(&packets);
    // Exclude repair packets from retransmit.
    let shreds: Vec<_> = packets
        .iter()
        .flat_map(PacketBatch::iter)
        .filter(|packet| !packet.meta().discard() && !packet.meta().repair())
        .filter_map(shred::layout::get_shred)
        .map(<[u8]>::to_vec)
        .collect();
    stats.num_retransmit_shreds += shreds.len();
    if !turbine_disabled.load(Ordering::Relaxed) {
        retransmit_sender.send(shreds)?;
        verified_sender.send(packets)?;
    }
    stats.elapsed_micros += now.elapsed().as_micros() as u64;
    Ok(())
}

fn verify_packets(
    self_pubkey: &Pubkey,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    recycler_cache: &RecyclerCache,
    packets: &mut [PacketBatch],
) {
    let working_bank = bank_forks.read().unwrap().working_bank();
    let leader_slots: HashMap<Slot, [u8; 32]> =
        get_slot_leaders(self_pubkey, packets, leader_schedule_cache, &working_bank)
            .into_iter()
            .filter_map(|(slot, pubkey)| Some((slot, pubkey?.to_bytes())))
            .chain(std::iter::once((Slot::MAX, [0u8; 32])))
            .collect();
    let out = verify_shreds_gpu(packets, &leader_slots, recycler_cache);
    solana_perf::sigverify::mark_disabled(packets, &out);
}

// Returns pubkey of leaders for shred slots refrenced in the packets.
// Marks packets as discard if:
//   - fails to deserialize the shred slot.
//   - slot leader is unknown.
//   - slot leader is the node itself (circular transmission).
fn get_slot_leaders(
    self_pubkey: &Pubkey,
    batches: &mut [PacketBatch],
    leader_schedule_cache: &LeaderScheduleCache,
    bank: &Bank,
) -> HashMap<Slot, Option<Pubkey>> {
    let mut leaders = HashMap::<Slot, Option<Pubkey>>::new();
    for batch in batches {
        for packet in batch.iter_mut() {
            if packet.meta().discard() {
                continue;
            }
            let shred = shred::layout::get_shred(packet);
            let slot = match shred.and_then(shred::layout::get_slot) {
                None => {
                    packet.meta_mut().set_discard(true);
                    continue;
                }
                Some(slot) => slot,
            };
            let leader = leaders.entry(slot).or_insert_with(|| {
                let leader = leader_schedule_cache.slot_leader_at(slot, Some(bank))?;
                // Discard the shred if the slot leader is the node itself.
                (&leader != self_pubkey).then_some(leader)
            });
            if leader.is_none() {
                packet.meta_mut().set_discard(true);
            }
        }
    }
    leaders
}

fn count_discards(packets: &[PacketBatch]) -> usize {
    packets
        .iter()
        .flat_map(PacketBatch::iter)
        .filter(|packet| packet.meta().discard())
        .count()
}

impl From<RecvTimeoutError> for Error {
    fn from(err: RecvTimeoutError) -> Self {
        match err {
            RecvTimeoutError::Timeout => Self::RecvTimeout,
            RecvTimeoutError::Disconnected => Self::RecvDisconnected,
        }
    }
}

impl<T> From<SendError<T>> for Error {
    fn from(_: SendError<T>) -> Self {
        Self::SendError
    }
}

struct ShredSigVerifyStats {
    since: Instant,
    num_iters: usize,
    num_packets: usize,
    num_discards_pre: usize,
    num_discards_post: usize,
    num_retransmit_shreds: usize,
    elapsed_micros: u64,
}

impl ShredSigVerifyStats {
    const METRICS_SUBMIT_CADENCE: Duration = Duration::from_secs(2);

    fn new(now: Instant) -> Self {
        Self {
            since: now,
            num_iters: 0usize,
            num_packets: 0usize,
            num_discards_pre: 0usize,
            num_discards_post: 0usize,
            num_retransmit_shreds: 0usize,
            elapsed_micros: 0u64,
        }
    }

    fn maybe_submit(&mut self) {
        if self.since.elapsed() <= Self::METRICS_SUBMIT_CADENCE {
            return;
        }
        datapoint_info!(
            "shred_sigverify",
            ("num_iters", self.num_iters, i64),
            ("num_packets", self.num_packets, i64),
            ("num_discards_pre", self.num_discards_pre, i64),
            ("num_discards_post", self.num_discards_post, i64),
            ("num_retransmit_shreds", self.num_retransmit_shreds, i64),
            ("elapsed_micros", self.elapsed_micros, i64),
        );
        *self = Self::new(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_ledger::{
            genesis_utils::create_genesis_config_with_leader,
            shred::{Shred, ShredFlags},
        },
        solana_perf::packet::Packet,
        solana_runtime::bank::Bank,
        solana_sdk::signature::{Keypair, Signer},
    };

    #[test]
    fn test_sigverify_shreds_verify_batches() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let bank = Bank::new_for_tests(
            &create_genesis_config_with_leader(100, &leader_pubkey, 10).genesis_config,
        );
        let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank);
        let bank_forks = RwLock::new(BankForks::new(bank));
        let batch_size = 2;
        let mut batch = PacketBatch::with_capacity(batch_size);
        batch.resize(batch_size, Packet::default());
        let mut batches = vec![batch];

        let mut shred = Shred::new_from_data(
            0,
            0xc0de,
            0xdead,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        shred.sign(&leader_keypair);
        batches[0][0].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][0].meta_mut().size = shred.payload().len();

        let mut shred = Shred::new_from_data(
            0,
            0xbeef,
            0xc0de,
            &[1, 2, 3, 4],
            ShredFlags::LAST_SHRED_IN_SLOT,
            0,
            0,
            0xc0de,
        );
        let wrong_keypair = Keypair::new();
        shred.sign(&wrong_keypair);
        batches[0][1].buffer_mut()[..shred.payload().len()].copy_from_slice(shred.payload());
        batches[0][1].meta_mut().size = shred.payload().len();

        verify_packets(
            &Pubkey::new_unique(), // self_pubkey
            &bank_forks,
            &leader_schedule_cache,
            &RecyclerCache::warmed(),
            &mut batches,
        );
        assert!(!batches[0][0].meta().discard());
        assert!(batches[0][1].meta().discard());
    }
}
