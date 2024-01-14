use {
    crate::{
        nonblocking::quic::ConnectionPeerType,
        quic::{StreamStats, MAX_UNSTAKED_CONNECTIONS},
    },
    percentage::Percentage,
    std::{
        cmp,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    },
};

/// Limit to 250K PPS
const MAX_STREAMS_PER_MS: u64 = 250;
const MAX_UNSTAKED_STREAMS_PERCENT: u64 = 20;
const STREAM_THROTTLING_INTERVAL_MS: u64 = 100;
pub const STREAM_STOP_CODE_THROTTLING: u32 = 15;
const STREAM_LOAD_EMA_INTERVAL_MS: u64 = 5;
const STREAM_LOAD_EMA_INTERVAL_COUNT: u64 = 10;
const MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION: u64 = 8;

pub(crate) struct StakedStreamLoadEMA {
    current_load_ema: AtomicU64,
    load_in_recent_interval: AtomicU64,
    last_update: RwLock<Instant>,
    stats: Arc<StreamStats>,
}

impl StakedStreamLoadEMA {
    pub(crate) fn new(stats: Arc<StreamStats>) -> Self {
        Self {
            current_load_ema: AtomicU64::default(),
            load_in_recent_interval: AtomicU64::default(),
            last_update: RwLock::new(Instant::now()),
            stats,
        }
    }

    fn ema_function(current_ema: u128, recent_load: u128) -> u128 {
        // Using the EMA multiplier helps in avoiding the floating point math during EMA related calculations
        const STREAM_LOAD_EMA_MULTIPLIER: u128 = 1024;
        let multiplied_smoothing_factor: u128 =
            2 * STREAM_LOAD_EMA_MULTIPLIER / (u128::from(STREAM_LOAD_EMA_INTERVAL_COUNT) + 1);

        // The formula is
        //    updated_ema = recent_load * smoothing_factor + current_ema * (1 - smoothing_factor)
        // To avoid floating point math, we are using STREAM_LOAD_EMA_MULTIPLIER
        //    updated_ema = (recent_load * multiplied_smoothing_factor
        //                   + current_ema * (multiplier - multiplied_smoothing_factor)) / multiplier
        (recent_load * multiplied_smoothing_factor
            + current_ema * (STREAM_LOAD_EMA_MULTIPLIER - multiplied_smoothing_factor))
            / STREAM_LOAD_EMA_MULTIPLIER
    }

    fn update_ema(&self, time_since_last_update_ms: u128) {
        // if time_since_last_update_ms > STREAM_LOAD_EMA_INTERVAL_MS, there might be intervals where ema was not updated.
        // count how many updates (1 + missed intervals) are needed.
        let num_extra_updates =
            time_since_last_update_ms.saturating_sub(1) / u128::from(STREAM_LOAD_EMA_INTERVAL_MS);

        let load_in_recent_interval =
            u128::from(self.load_in_recent_interval.swap(0, Ordering::Relaxed));

        let mut updated_load_ema = Self::ema_function(
            u128::from(self.current_load_ema.load(Ordering::Relaxed)),
            load_in_recent_interval,
        );

        for _ in 0..num_extra_updates {
            updated_load_ema = Self::ema_function(updated_load_ema, load_in_recent_interval);
        }

        let Ok(updated_load_ema) = u64::try_from(updated_load_ema) else {
            error!(
                "Failed to convert EMA {} to a u64. Not updating the load EMA",
                updated_load_ema
            );
            self.stats
                .stream_load_ema_overflow
                .fetch_add(1, Ordering::Relaxed);
            return;
        };

        self.current_load_ema
            .store(updated_load_ema, Ordering::Relaxed);
        self.stats
            .stream_load_ema
            .store(updated_load_ema as usize, Ordering::Relaxed);
    }

    pub(crate) fn update_ema_if_needed(&self) {
        const EMA_DURATION: Duration = Duration::from_millis(STREAM_LOAD_EMA_INTERVAL_MS);
        // Read lock enables multiple connection handlers to run in parallel if interval is not expired
        if Instant::now().duration_since(*self.last_update.read().unwrap()) >= EMA_DURATION {
            let mut last_update_w = self.last_update.write().unwrap();
            // Recheck as some other thread might have updated the ema since this thread tried to acquire the write lock.
            let since_last_update = Instant::now().duration_since(*last_update_w);
            if since_last_update >= EMA_DURATION {
                *last_update_w = Instant::now();
                self.update_ema(since_last_update.as_millis());
            }
        }
    }

    pub(crate) fn increment_load(&self) {
        self.load_in_recent_interval.fetch_add(1, Ordering::Relaxed);
        self.update_ema_if_needed();
    }

    pub(crate) fn available_load_capacity_in_throttling_duration(
        &self,
        stake: u64,
        total_stake: u64,
    ) -> u64 {
        let ema_window_ms = STREAM_LOAD_EMA_INTERVAL_MS * STREAM_LOAD_EMA_INTERVAL_COUNT;
        let max_load_in_ema_window = u128::from(
            (MAX_STREAMS_PER_MS
                - Percentage::from(MAX_UNSTAKED_STREAMS_PERCENT).apply_to(MAX_STREAMS_PER_MS))
                * ema_window_ms,
        );

        // If the current load is low, cap it to 25% of max_load.
        let current_load = cmp::max(
            u128::from(self.current_load_ema.load(Ordering::Relaxed)),
            max_load_in_ema_window / 4,
        );

        // Formula is (max_load ^ 2 / current_load) * (stake / total_stake)
        let capacity_in_ema_window =
            (max_load_in_ema_window * max_load_in_ema_window * u128::from(stake))
                / (current_load * u128::from(total_stake));

        let calculated_capacity = capacity_in_ema_window
            * u128::from(STREAM_THROTTLING_INTERVAL_MS)
            / u128::from(ema_window_ms);
        let calculated_capacity = u64::try_from(calculated_capacity).unwrap_or_else(|_| {
            error!(
                "Failed to convert stream capacity {} to u64. Using minimum load capacity",
                calculated_capacity
            );
            self.stats
                .stream_load_capacity_overflow
                .fetch_add(1, Ordering::Relaxed);
            MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION
        });

        cmp::max(
            calculated_capacity,
            MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION,
        )
    }
}

pub(crate) fn max_streams_for_connection_in_throttling_duration(
    peer_type: ConnectionPeerType,
    total_stake: u64,
    ema_load: Arc<StakedStreamLoadEMA>,
) -> u64 {
    match peer_type {
        ConnectionPeerType::Unstaked => {
            let max_num_connections =
                u64::try_from(MAX_UNSTAKED_CONNECTIONS).unwrap_or_else(|_| {
                    error!(
                        "Failed to convert maximum number of unstaked connections {} to u64.",
                        MAX_UNSTAKED_CONNECTIONS
                    );
                    500
                });
            Percentage::from(MAX_UNSTAKED_STREAMS_PERCENT)
                .apply_to(MAX_STREAMS_PER_MS * STREAM_THROTTLING_INTERVAL_MS)
                .saturating_div(max_num_connections)
        }
        ConnectionPeerType::Staked(stake) => {
            ema_load.available_load_capacity_in_throttling_duration(stake, total_stake)
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionStreamCounter {
    pub(crate) stream_count: AtomicU64,
    last_throttling_instant: RwLock<tokio::time::Instant>,
}

impl ConnectionStreamCounter {
    pub(crate) fn new() -> Self {
        Self {
            stream_count: AtomicU64::default(),
            last_throttling_instant: RwLock::new(tokio::time::Instant::now()),
        }
    }

    pub(crate) fn reset_throttling_params_if_needed(&self) {
        const THROTTLING_INTERVAL: Duration = Duration::from_millis(STREAM_THROTTLING_INTERVAL_MS);
        if tokio::time::Instant::now().duration_since(*self.last_throttling_instant.read().unwrap())
            > THROTTLING_INTERVAL
        {
            let mut last_throttling_instant = self.last_throttling_instant.write().unwrap();
            // Recheck as some other thread might have done throttling since this thread tried to acquire the write lock.
            if tokio::time::Instant::now().duration_since(*last_throttling_instant)
                > THROTTLING_INTERVAL
            {
                *last_throttling_instant = tokio::time::Instant::now();
                self.stream_count.store(0, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            nonblocking::stream_throttle::{
                max_streams_for_connection_in_throttling_duration,
                MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION,
                STREAM_LOAD_EMA_INTERVAL_MS,
            },
            quic::StreamStats,
        },
        std::{
            sync::{atomic::Ordering, Arc},
            time::{Duration, Instant},
        },
    };

    #[test]
    fn test_max_streams_for_unstaked_connection() {
        let load_ema = Arc::new(StakedStreamLoadEMA::new(Arc::new(StreamStats::default())));
        // 25K packets per ms * 20% / 500 max unstaked connections
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Unstaked,
                10000,
                load_ema.clone(),
            ),
            10
        );
    }
    #[test]
    fn test_max_streams_for_staked_connection() {
        let load_ema = Arc::new(StakedStreamLoadEMA::new(Arc::new(StreamStats::default())));

        // EMA load is used for staked connections to calculate max number of allowed streams.
        // EMA window = 5ms interval * 10 intervals = 50ms
        // max streams per window = 250K streams/sec * 80% = 200K/sec = 10K per 50ms
        // max_streams in 50ms = ((10K * 10K) / ema_load) * stake / total_stake
        //
        // Stream throttling window is 100ms. So it'll double the amount of max streams.
        // max_streams in 100ms (throttling window) = 2 * ((10K * 10K) / ema_load) * stake / total_stake

        load_ema.current_load_ema.store(10000, Ordering::Relaxed);
        // ema_load = 10K, stake = 15, total_stake = 10K
        // max_streams in 100ms (throttling window) = 2 * ((10K * 10K) / 10K) * 15 / 10K  = 30
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(15),
                10000,
                load_ema.clone(),
            ),
            30
        );

        // ema_load = 10K, stake = 1K, total_stake = 10K
        // max_streams in 100ms (throttling window) = 2 * ((10K * 10K) / 10K) * 1K / 10K  = 2K
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(1000),
                10000,
                load_ema.clone(),
            ),
            2000
        );

        load_ema.current_load_ema.store(2500, Ordering::Relaxed);
        // ema_load = 2.5K, stake = 15, total_stake = 10K
        // max_streams in 100ms (throttling window) = 2 * ((10K * 10K) / 2.5K) * 15 / 10K  = 120
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(15),
                10000,
                load_ema.clone(),
            ),
            120
        );

        // ema_load = 2.5K, stake = 1K, total_stake = 10K
        // max_streams in 100ms (throttling window) = 2 * ((10K * 10K) / 2.5K) * 1K / 10K  = 8000
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(1000),
                10000,
                load_ema.clone(),
            ),
            8000
        );

        // At 2000, the load is less than 25% of max_load (10K).
        // Test that we cap it to 25%, yielding the same result as if load was 2500.
        load_ema.current_load_ema.store(2000, Ordering::Relaxed);
        // function = ((10K * 10K) / 25% of 10K) * stake / total_stake
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(15),
                10000,
                load_ema.clone(),
            ),
            120
        );

        // function = ((10K * 10K) / 25% of 10K) * stake / total_stake
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(1000),
                10000,
                load_ema.clone(),
            ),
            8000
        );

        // At 1/40000 stake weight, and minimum load, it should still allow
        // MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION of streams.
        assert_eq!(
            max_streams_for_connection_in_throttling_duration(
                ConnectionPeerType::Staked(1),
                40000,
                load_ema.clone(),
            ),
            MIN_STREAMS_PER_THROTTLING_INTERVAL_FOR_STAKED_CONNECTION
        );
    }

    #[test]
    fn test_update_ema() {
        let stream_load_ema = Arc::new(StakedStreamLoadEMA::new(Arc::new(StreamStats::default())));
        stream_load_ema
            .load_in_recent_interval
            .store(2500, Ordering::Relaxed);
        stream_load_ema
            .current_load_ema
            .store(2000, Ordering::Relaxed);

        stream_load_ema.update_ema(5);

        let updated_ema = stream_load_ema.current_load_ema.load(Ordering::Relaxed);
        assert_eq!(updated_ema, 2090);

        stream_load_ema
            .load_in_recent_interval
            .store(2500, Ordering::Relaxed);

        stream_load_ema.update_ema(5);

        let updated_ema = stream_load_ema.current_load_ema.load(Ordering::Relaxed);
        assert_eq!(updated_ema, 2164);
    }

    #[test]
    fn test_update_ema_missing_interval() {
        let stream_load_ema = Arc::new(StakedStreamLoadEMA::new(Arc::new(StreamStats::default())));
        stream_load_ema
            .load_in_recent_interval
            .store(2500, Ordering::Relaxed);
        stream_load_ema
            .current_load_ema
            .store(2000, Ordering::Relaxed);

        stream_load_ema.update_ema(8);

        let updated_ema = stream_load_ema.current_load_ema.load(Ordering::Relaxed);
        assert_eq!(updated_ema, 2164);
    }

    #[test]
    fn test_update_ema_if_needed() {
        let stream_load_ema = Arc::new(StakedStreamLoadEMA::new(Arc::new(StreamStats::default())));
        stream_load_ema
            .load_in_recent_interval
            .store(2500, Ordering::Relaxed);
        stream_load_ema
            .current_load_ema
            .store(2000, Ordering::Relaxed);

        stream_load_ema.update_ema_if_needed();

        let updated_ema = stream_load_ema.current_load_ema.load(Ordering::Relaxed);
        assert_eq!(updated_ema, 2000);

        let ema_interval = Duration::from_millis(STREAM_LOAD_EMA_INTERVAL_MS);
        *stream_load_ema.last_update.write().unwrap() =
            Instant::now().checked_sub(ema_interval).unwrap();

        stream_load_ema.update_ema_if_needed();
        assert!(
            Instant::now().duration_since(*stream_load_ema.last_update.read().unwrap())
                < ema_interval
        );

        let updated_ema = stream_load_ema.current_load_ema.load(Ordering::Relaxed);
        assert_eq!(updated_ema, 2090);
    }
}
