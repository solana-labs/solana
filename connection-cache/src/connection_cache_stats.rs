use {
    crate::client_connection::ClientStats,
    std::sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub struct ConnectionCacheStats {
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub cache_evictions: AtomicU64,
    pub eviction_time_ms: AtomicU64,
    pub sent_packets: AtomicU64,
    pub total_batches: AtomicU64,
    pub batch_success: AtomicU64,
    pub batch_failure: AtomicU64,
    pub get_connection_ms: AtomicU64,
    pub get_connection_lock_ms: AtomicU64,
    pub get_connection_hit_ms: AtomicU64,
    pub get_connection_miss_ms: AtomicU64,

    // Need to track these separately per-connection
    // because we need to track the base stat value from quinn
    pub total_client_stats: ClientStats,
}

pub const CONNECTION_STAT_SUBMISSION_INTERVAL: u64 = 2000;

impl ConnectionCacheStats {
    pub fn add_client_stats(
        &self,
        client_stats: &ClientStats,
        num_packets: usize,
        is_success: bool,
    ) {
        self.total_client_stats.total_connections.fetch_add(
            client_stats.total_connections.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.connection_reuse.fetch_add(
            client_stats.connection_reuse.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.connection_errors.fetch_add(
            client_stats.connection_errors.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.zero_rtt_accepts.fetch_add(
            client_stats.zero_rtt_accepts.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.zero_rtt_rejects.fetch_add(
            client_stats.zero_rtt_rejects.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.make_connection_ms.fetch_add(
            client_stats.make_connection_ms.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.send_timeout.fetch_add(
            client_stats.send_timeout.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.send_packets_us.fetch_add(
            client_stats.send_packets_us.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.successful_packets.fetch_add(
            client_stats.successful_packets.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.total_client_stats.prepare_connection_us.fetch_add(
            client_stats.prepare_connection_us.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.sent_packets
            .fetch_add(num_packets as u64, Ordering::Relaxed);
        self.total_batches.fetch_add(1, Ordering::Relaxed);
        if is_success {
            self.batch_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.batch_failure.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn report(&self, name: &'static str) {
        let successful_packets = self
            .total_client_stats
            .successful_packets
            .swap(0, Ordering::Relaxed);

        let (average_send_packet_us, average_prepare_connection_us) = if successful_packets > 0 {
            (
                self.total_client_stats
                    .send_packets_us
                    .swap(0, Ordering::Relaxed)
                    / successful_packets,
                self.total_client_stats
                    .prepare_connection_us
                    .swap(0, Ordering::Relaxed)
                    / successful_packets,
            )
        } else {
            (0, 0)
        };

        datapoint_info!(
            name,
            (
                "cache_hits",
                self.cache_hits.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_misses",
                self.cache_misses.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "cache_evictions",
                self.cache_evictions.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "eviction_time_ms",
                self.eviction_time_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_ms",
                self.get_connection_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_lock_ms",
                self.get_connection_lock_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_hit_ms",
                self.get_connection_hit_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "get_connection_miss_ms",
                self.get_connection_miss_ms.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "make_connection_ms",
                self.total_client_stats
                    .make_connection_ms
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "total_connections",
                self.total_client_stats
                    .total_connections
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_reuse",
                self.total_client_stats
                    .connection_reuse
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "connection_errors",
                self.total_client_stats
                    .connection_errors
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "zero_rtt_accepts",
                self.total_client_stats
                    .zero_rtt_accepts
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "zero_rtt_rejects",
                self.total_client_stats
                    .zero_rtt_rejects
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "congestion_events",
                self.total_client_stats.congestion_events.load_and_reset(),
                i64
            ),
            (
                "streams_blocked_uni",
                self.total_client_stats.streams_blocked_uni.load_and_reset(),
                i64
            ),
            (
                "data_blocked",
                self.total_client_stats.data_blocked.load_and_reset(),
                i64
            ),
            ("acks", self.total_client_stats.acks.load_and_reset(), i64),
            (
                "num_packets",
                self.sent_packets.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "total_batches",
                self.total_batches.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "batch_failure",
                self.batch_failure.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "send_timeout",
                self.total_client_stats
                    .send_timeout
                    .swap(0, Ordering::Relaxed),
                i64
            ),
            ("average_send_packet_us", average_send_packet_us, i64),
            ("successful_packets", successful_packets, i64),
            (
                "average_prepare_connection_us",
                average_prepare_connection_us,
                i64
            ),
        );
    }
}
