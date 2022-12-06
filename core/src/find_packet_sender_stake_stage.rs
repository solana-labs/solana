use {
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    solana_measure::measure::Measure,
    solana_perf::packet::PacketBatch,
    solana_sdk::timing::timestamp,
    solana_streamer::streamer::{self, StakedNodes, StreamerError},
    std::{
        collections::HashMap,
        net::IpAddr,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
};

// Try to target 50ms, rough timings from a testnet validator
//
// 50ms/(200ns/packet) = 250k packets
const MAX_FINDPACKETSENDERSTAKE_BATCH: usize = 250_000;

pub type FindPacketSenderStakeSender = Sender<Vec<PacketBatch>>;
pub type FindPacketSenderStakeReceiver = Receiver<Vec<PacketBatch>>;

#[derive(Debug, Default)]
struct FindPacketSenderStakeStats {
    last_print: u64,
    refresh_ip_to_stake_time: u64,
    apply_sender_stakes_time: u64,
    send_batches_time: u64,
    receive_batches_time: u64,
    total_batches: u64,
    total_packets: u64,
    total_discard_random: usize,
    total_discard_random_time_us: usize,
}

impl FindPacketSenderStakeStats {
    fn report(&mut self, name: &'static str) {
        let now = timestamp();
        let elapsed_ms = now - self.last_print;
        if elapsed_ms > 2000 {
            datapoint_info!(
                name,
                (
                    "refresh_ip_to_stake_time_us",
                    self.refresh_ip_to_stake_time as i64,
                    i64
                ),
                (
                    "apply_sender_stakes_time_us",
                    self.apply_sender_stakes_time as i64,
                    i64
                ),
                ("send_batches_time_us", self.send_batches_time as i64, i64),
                (
                    "receive_batches_time_ns",
                    self.receive_batches_time as i64,
                    i64
                ),
                ("total_batches", self.total_batches as i64, i64),
                ("total_packets", self.total_packets as i64, i64),
                ("total_discard_random", self.total_discard_random, i64),
                (
                    "total_discard_random_time_us",
                    self.total_discard_random_time_us,
                    i64
                ),
            );
            *self = FindPacketSenderStakeStats::default();
            self.last_print = now;
        }
    }
}

pub struct FindPacketSenderStakeStage {
    thread_hdl: JoinHandle<()>,
}

impl FindPacketSenderStakeStage {
    pub fn new(
        packet_receiver: streamer::PacketBatchReceiver,
        sender: FindPacketSenderStakeSender,
        staked_nodes: Arc<RwLock<StakedNodes>>,
        name: &'static str,
    ) -> Self {
        let mut stats = FindPacketSenderStakeStats::default();
        let thread_hdl = Builder::new()
            .name("solPktStake".to_string())
            .spawn(move || loop {
                match streamer::recv_packet_batches(&packet_receiver) {
                    Ok((mut batches, num_packets, recv_duration)) => {
                        let num_batches = batches.len();

                        let mut discard_random_time =
                            Measure::start("findpacketsenderstake_discard_random_time");
                        let non_discarded_packets = solana_perf::discard::discard_batches_randomly(
                            &mut batches,
                            MAX_FINDPACKETSENDERSTAKE_BATCH,
                            num_packets,
                        );
                        let num_discarded_randomly =
                            num_packets.saturating_sub(non_discarded_packets);
                        discard_random_time.stop();

                        let mut apply_sender_stakes_time =
                            Measure::start("apply_sender_stakes_time");
                        let mut apply_stake = || {
                            let ip_to_stake = staked_nodes.read().unwrap();
                            Self::apply_sender_stakes(&mut batches, &ip_to_stake.ip_stake_map);
                        };
                        apply_stake();
                        apply_sender_stakes_time.stop();

                        let mut send_batches_time = Measure::start("send_batches_time");
                        if let Err(e) = sender.send(batches) {
                            info!("Sender error: {:?}", e);
                        }
                        send_batches_time.stop();

                        stats.apply_sender_stakes_time = stats
                            .apply_sender_stakes_time
                            .saturating_add(apply_sender_stakes_time.as_us());
                        stats.send_batches_time = stats
                            .send_batches_time
                            .saturating_add(send_batches_time.as_us());
                        stats.receive_batches_time = stats
                            .receive_batches_time
                            .saturating_add(recv_duration.as_nanos() as u64);
                        stats.total_batches =
                            stats.total_batches.saturating_add(num_batches as u64);
                        stats.total_packets =
                            stats.total_packets.saturating_add(num_packets as u64);
                        stats.total_discard_random_time_us += discard_random_time.as_us() as usize;
                        stats.total_discard_random += num_discarded_randomly;
                    }
                    Err(e) => match e {
                        StreamerError::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                        StreamerError::RecvTimeout(RecvTimeoutError::Timeout) => (),
                        _ => error!("error: {:?}", e),
                    },
                }

                stats.report(name);
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn apply_sender_stakes(batches: &mut [PacketBatch], ip_to_stake: &HashMap<IpAddr, u64>) {
        batches
            .iter_mut()
            .flat_map(|batch| batch.iter_mut())
            .for_each(|packet| {
                packet.meta_mut().sender_stake = ip_to_stake
                    .get(&packet.meta().addr)
                    .copied()
                    .unwrap_or_default();
            });
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
