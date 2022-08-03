use {
    solana_sdk::clock::Slot,
    std::{
        ops::AddAssign,
        time::{Duration, Instant},
    },
};

#[derive(Default, Clone, Copy)]
pub struct ProcessShredsStats {
    // Per-slot elapsed time
    pub shredding_elapsed: u64,
    pub receive_elapsed: u64,
    pub serialize_elapsed: u64,
    pub gen_data_elapsed: u64,
    pub gen_coding_elapsed: u64,
    pub sign_coding_elapsed: u64,
    pub coding_send_elapsed: u64,
    pub get_leader_schedule_elapsed: u64,
<<<<<<< HEAD:ledger/src/shred_stats.rs
    // Number of data shreds from serializing ledger entries which do not make
    // a full batch of MAX_DATA_SHREDS_PER_FEC_BLOCK; counted in 4 buckets.
    num_residual_data_shreds: [usize; 4],
=======
    // Histogram count of num_data_shreds obtained from serializing entries
    // counted in 5 buckets.
    num_data_shreds_hist: [usize; 5],
    // If the blockstore already has shreds for the broadcast slot.
    pub num_extant_slots: u64,
>>>>>>> 403b2e484 (records num data shreds obtained from serializing entries (#26888)):ledger/src/shred/stats.rs
    pub(crate) data_buffer_residual: usize,
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct ShredFetchStats {
    pub index_overrun: usize,
    pub shred_count: usize,
    pub ping_count: usize,
    pub ping_err_verify_count: usize,
    pub(crate) index_bad_deserialize: usize,
    pub(crate) index_out_of_bounds: usize,
    pub(crate) slot_bad_deserialize: usize,
    pub duplicate_shred: usize,
    pub slot_out_of_range: usize,
    pub(crate) bad_shred_type: usize,
    since: Option<Instant>,
}

impl ProcessShredsStats {
    pub fn submit(
        &mut self,
        name: &'static str,
        slot: Slot,
        num_data_shreds: u32,
        slot_broadcast_time: Option<Duration>,
    ) {
        let slot_broadcast_time = slot_broadcast_time
            .map(|t| t.as_micros() as i64)
            .unwrap_or(-1);
        self.num_data_shreds_hist.iter_mut().fold(0, |acc, num| {
            *num += acc;
            *num
        });
        datapoint_info!(
            name,
            ("slot", slot, i64),
            ("shredding_time", self.shredding_elapsed, i64),
            ("receive_time", self.receive_elapsed, i64),
            ("num_data_shreds", num_data_shreds, i64),
            ("slot_broadcast_time", slot_broadcast_time, i64),
            (
                "get_leader_schedule_time",
                self.get_leader_schedule_elapsed,
                i64
            ),
            ("serialize_shreds_time", self.serialize_elapsed, i64),
            ("gen_data_time", self.gen_data_elapsed, i64),
            ("gen_coding_time", self.gen_coding_elapsed, i64),
            ("sign_coding_time", self.sign_coding_elapsed, i64),
            ("coding_send_time", self.coding_send_elapsed, i64),
            ("data_buffer_residual", self.data_buffer_residual, i64),
            ("num_data_shreds_07", self.num_data_shreds_hist[0], i64),
            ("num_data_shreds_15", self.num_data_shreds_hist[1], i64),
            ("num_data_shreds_31", self.num_data_shreds_hist[2], i64),
            ("num_data_shreds_63", self.num_data_shreds_hist[3], i64),
            ("num_data_shreds_64", self.num_data_shreds_hist[4], i64),
        );
        *self = Self::default();
    }

    pub(crate) fn record_num_data_shreds(&mut self, num_data_shreds: usize) {
        let index = usize::BITS - num_data_shreds.leading_zeros();
        let index = index.saturating_sub(3) as usize;
        let index = index.min(self.num_data_shreds_hist.len() - 1);
        self.num_data_shreds_hist[index] += 1;
    }
}

impl ShredFetchStats {
    pub fn maybe_submit(&mut self, name: &'static str, cadence: Duration) {
        let elapsed = self.since.as_ref().map(Instant::elapsed);
        if elapsed.unwrap_or(Duration::MAX) < cadence {
            return;
        }
        datapoint_info!(
            name,
            ("index_overrun", self.index_overrun, i64),
            ("shred_count", self.shred_count, i64),
            ("ping_count", self.ping_count, i64),
            ("ping_err_verify_count", self.ping_err_verify_count, i64),
            ("slot_bad_deserialize", self.slot_bad_deserialize, i64),
            ("index_bad_deserialize", self.index_bad_deserialize, i64),
            ("index_out_of_bounds", self.index_out_of_bounds, i64),
            ("slot_out_of_range", self.slot_out_of_range, i64),
            ("duplicate_shred", self.duplicate_shred, i64),
        );
        *self = Self {
            since: Some(Instant::now()),
            ..Self::default()
        };
    }
}

impl AddAssign<ProcessShredsStats> for ProcessShredsStats {
    fn add_assign(&mut self, rhs: Self) {
        let Self {
            shredding_elapsed,
            receive_elapsed,
            serialize_elapsed,
            gen_data_elapsed,
            gen_coding_elapsed,
            sign_coding_elapsed,
            coding_send_elapsed,
            get_leader_schedule_elapsed,
<<<<<<< HEAD:ledger/src/shred_stats.rs
            num_residual_data_shreds,
=======
            num_data_shreds_hist,
            num_extant_slots,
>>>>>>> 403b2e484 (records num data shreds obtained from serializing entries (#26888)):ledger/src/shred/stats.rs
            data_buffer_residual,
        } = rhs;
        self.shredding_elapsed += shredding_elapsed;
        self.receive_elapsed += receive_elapsed;
        self.serialize_elapsed += serialize_elapsed;
        self.gen_data_elapsed += gen_data_elapsed;
        self.gen_coding_elapsed += gen_coding_elapsed;
        self.sign_coding_elapsed += sign_coding_elapsed;
        self.coding_send_elapsed += coding_send_elapsed;
        self.get_leader_schedule_elapsed += get_leader_schedule_elapsed;
        self.data_buffer_residual += data_buffer_residual;
        for (i, bucket) in self.num_data_shreds_hist.iter_mut().enumerate() {
            *bucket += num_data_shreds_hist[i];
        }
    }
}
