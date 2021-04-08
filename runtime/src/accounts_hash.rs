use log::*;
use rayon::prelude::*;
use solana_measure::measure::Measure;
use solana_sdk::{
    clock::Slot,
    hash::{Hash, Hasher},
    pubkey::Pubkey,
};
use std::{convert::TryInto, sync::Mutex};

pub const ZERO_RAW_LAMPORTS_SENTINEL: u64 = std::u64::MAX;
pub const MERKLE_FANOUT: usize = 16;

#[derive(Default, Debug)]
pub struct PreviousPass {
    pub reduced_hashes: Vec<Vec<Hash>>,
    pub remaining_unhashed: Vec<Hash>,
    pub lamports: u64,
}

#[derive(Debug, Default)]
pub struct HashStats {
    pub scan_time_total_us: u64,
    pub zeros_time_total_us: u64,
    pub hash_time_total_us: u64,
    pub sort_time_total_us: u64,
    pub flatten_time_total_us: u64,
    pub pre_scan_flatten_time_total_us: u64,
    pub hash_total: usize,
    pub unreduced_entries: usize,
    pub num_snapshot_storage: usize,
}
impl HashStats {
    fn log(&mut self) {
        let total_time_us = self.scan_time_total_us
            + self.zeros_time_total_us
            + self.hash_time_total_us
            + self.sort_time_total_us
            + self.flatten_time_total_us
            + self.pre_scan_flatten_time_total_us;
        datapoint_info!(
            "calculate_accounts_hash_without_index",
            ("accounts_scan", self.scan_time_total_us, i64),
            ("eliminate_zeros", self.zeros_time_total_us, i64),
            ("hash", self.hash_time_total_us, i64),
            ("sort", self.sort_time_total_us, i64),
            ("hash_total", self.hash_total, i64),
            ("flatten", self.flatten_time_total_us, i64),
            ("unreduced_entries", self.unreduced_entries as i64, i64),
            (
                "num_snapshot_storage",
                self.num_snapshot_storage as i64,
                i64
            ),
            (
                "pre_scan_flatten",
                self.pre_scan_flatten_time_total_us as i64,
                i64
            ),
            ("total", total_time_us as i64, i64),
        );
    }
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct CalculateHashIntermediate {
    pub version: u64,
    pub hash: Hash,
    pub lamports: u64,
    pub slot: Slot,
    pub pubkey: Pubkey,
}

impl CalculateHashIntermediate {
    pub fn new(version: u64, hash: Hash, lamports: u64, slot: Slot, pubkey: Pubkey) -> Self {
        Self {
            version,
            hash,
            lamports,
            slot,
            pubkey,
        }
    }
}

#[derive(Default, Debug)]
pub struct CumulativeOffset {
    pub index: Vec<usize>,
    pub start_offset: usize,
}

impl CumulativeOffset {
    pub fn new(index: Vec<usize>, start_offset: usize) -> CumulativeOffset {
        Self {
            index,
            start_offset,
        }
    }
}

// Allow retrieving &[start..end] from a logical src: Vec<T>, where src is really Vec<Vec<T>> (or later Vec<Vec<Vec<T>>>)
// This model prevents callers from having to flatten which saves both working memory and time.
#[derive(Default, Debug)]
pub struct CumulativeOffsets {
    cumulative_offsets: Vec<CumulativeOffset>,
    total_count: usize,
}

impl CumulativeOffsets {
    pub fn from_raw<T>(raw: &[Vec<T>]) -> CumulativeOffsets {
        let mut total_count: usize = 0;
        let cumulative_offsets: Vec<_> = raw
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let len = v.len();
                if len > 0 {
                    let result = CumulativeOffset::new(vec![i], total_count);
                    total_count += len;
                    Some(result)
                } else {
                    None
                }
            })
            .collect();

        Self {
            cumulative_offsets,
            total_count,
        }
    }

    pub fn from_raw_2d<T>(raw: &[Vec<Vec<T>>]) -> CumulativeOffsets {
        let mut total_count: usize = 0;
        let mut cumulative_offsets = Vec::with_capacity(0);
        for (i, v_outer) in raw.iter().enumerate() {
            for (j, v) in v_outer.iter().enumerate() {
                let len = v.len();
                if len > 0 {
                    if cumulative_offsets.is_empty() {
                        // the first inner, non-empty vector we find gives us an approximate rectangular shape
                        cumulative_offsets = Vec::with_capacity(raw.len() * v_outer.len());
                    }
                    cumulative_offsets.push(CumulativeOffset::new(vec![i, j], total_count));
                    total_count += len;
                }
            }
        }

        Self {
            cumulative_offsets,
            total_count,
        }
    }

    // return the biggest slice possible that starts at 'start'
    pub fn get_slice<'a, T>(&self, raw: &'a [Vec<T>], start: usize) -> &'a [T] {
        // This could be binary search, but we expect a small number of vectors.
        for i in (0..self.cumulative_offsets.len()).into_iter().rev() {
            let index = &self.cumulative_offsets[i];
            if start >= index.start_offset {
                let start = start - index.start_offset;
                const DIMENSION: usize = 0;
                return &raw[index.index[DIMENSION]][start..];
            }
        }
        panic!(
            "get_slice didn't find: {}, len: {}",
            start, self.total_count
        );
    }

    // return the biggest slice possible that starts at 'start'
    pub fn get_slice_2d<'a, T>(&self, raw: &'a [Vec<Vec<T>>], start: usize) -> &'a [T] {
        // This could be binary search, but we expect a small number of vectors.
        for i in (0..self.cumulative_offsets.len()).into_iter().rev() {
            let index = &self.cumulative_offsets[i];
            if start >= index.start_offset {
                let start = start - index.start_offset;
                const DIMENSION_0: usize = 0;
                const DIMENSION_1: usize = 1;
                return &raw[index.index[DIMENSION_0]][index.index[DIMENSION_1]][start..];
            }
        }
        panic!(
            "get_slice didn't find: {}, len: {}",
            start, self.total_count
        );
    }
}

#[derive(Debug)]
pub struct AccountsHash {
    pub dummy: i32,
}

impl AccountsHash {
    pub fn calculate_hash(hashes: Vec<Vec<Hash>>) -> (Hash, usize) {
        let cumulative_offsets = CumulativeOffsets::from_raw(&hashes);

        let hash_total = cumulative_offsets.total_count;
        let result = AccountsHash::compute_merkle_root_from_slices(
            hash_total,
            MERKLE_FANOUT,
            None,
            |start: usize| cumulative_offsets.get_slice(&hashes, start),
            None,
        );
        (result.0, hash_total)
    }

    pub fn compute_merkle_root(hashes: Vec<(Pubkey, Hash)>, fanout: usize) -> Hash {
        Self::compute_merkle_root_loop(hashes, fanout, |t| t.1)
    }

    // this function avoids an infinite recursion compiler error
    pub fn compute_merkle_root_recurse(hashes: Vec<Hash>, fanout: usize) -> Hash {
        Self::compute_merkle_root_loop(hashes, fanout, |t: &Hash| *t)
    }

    pub fn div_ceil(x: usize, y: usize) -> usize {
        let mut result = x / y;
        if x % y != 0 {
            result += 1;
        }
        result
    }

    // For the first iteration, there could be more items in the tuple than just hash and lamports.
    // Using extractor allows us to avoid an unnecessary array copy on the first iteration.
    pub fn compute_merkle_root_loop<T, F>(hashes: Vec<T>, fanout: usize, extractor: F) -> Hash
    where
        F: Fn(&T) -> Hash + std::marker::Sync,
        T: std::marker::Sync,
    {
        if hashes.is_empty() {
            return Hasher::default().result();
        }

        let mut time = Measure::start("time");

        let total_hashes = hashes.len();
        let chunks = Self::div_ceil(total_hashes, fanout);

        let result: Vec<_> = (0..chunks)
            .into_par_iter()
            .map(|i| {
                let start_index = i * fanout;
                let end_index = std::cmp::min(start_index + fanout, total_hashes);

                let mut hasher = Hasher::default();
                for item in hashes.iter().take(end_index).skip(start_index) {
                    let h = extractor(&item);
                    hasher.hash(h.as_ref());
                }

                hasher.result()
            })
            .collect();
        time.stop();
        debug!("hashing {} {}", total_hashes, time);

        if result.len() == 1 {
            result[0]
        } else {
            Self::compute_merkle_root_recurse(result, fanout)
        }
    }

    fn calculate_three_level_chunks(
        total_hashes: usize,
        fanout: usize,
        max_levels_per_pass: Option<usize>,
        specific_level_count: Option<usize>,
    ) -> (usize, usize, bool) {
        const THREE_LEVEL_OPTIMIZATION: usize = 3; // this '3' is dependent on the code structure below where we manually unroll
        let target = fanout.pow(THREE_LEVEL_OPTIMIZATION as u32);

        // Only use the 3 level optimization if we have at least 4 levels of data.
        // Otherwise, we'll be serializing a parallel operation.
        let threshold = target * fanout;
        let mut three_level = max_levels_per_pass.unwrap_or(usize::MAX) >= THREE_LEVEL_OPTIMIZATION
            && total_hashes >= threshold;
        if three_level {
            if let Some(specific_level_count_value) = specific_level_count {
                three_level = specific_level_count_value >= THREE_LEVEL_OPTIMIZATION;
            }
        }
        let (num_hashes_per_chunk, levels_hashed) = if three_level {
            (target, THREE_LEVEL_OPTIMIZATION)
        } else {
            (fanout, 1)
        };
        (num_hashes_per_chunk, levels_hashed, three_level)
    }

    // This function is designed to allow hashes to be located in multiple, perhaps multiply deep vecs.
    // The caller provides a function to return a slice from the source data.
    pub fn compute_merkle_root_from_slices<'a, F>(
        total_hashes: usize,
        fanout: usize,
        max_levels_per_pass: Option<usize>,
        get_hash_slice_starting_at_index: F,
        specific_level_count: Option<usize>,
    ) -> (Hash, Vec<Hash>)
    where
        F: Fn(usize) -> &'a [Hash] + std::marker::Sync,
    {
        if total_hashes == 0 {
            return (Hasher::default().result(), vec![]);
        }

        let mut time = Measure::start("time");

        let (num_hashes_per_chunk, levels_hashed, three_level) = Self::calculate_three_level_chunks(
            total_hashes,
            fanout,
            max_levels_per_pass,
            specific_level_count,
        );

        let chunks = Self::div_ceil(total_hashes, num_hashes_per_chunk);

        // initial fetch - could return entire slice
        let data: &[Hash] = get_hash_slice_starting_at_index(0);
        let data_len = data.len();

        let result: Vec<_> = (0..chunks)
            .into_par_iter()
            .map(|i| {
                // summary:
                // this closure computes 1 or 3 levels of merkle tree (all chunks will be 1 or all will be 3)
                // for a subset (our chunk) of the input data [start_index..end_index]

                // index into get_hash_slice_starting_at_index where this chunk's range begins
                let start_index = i * num_hashes_per_chunk;
                // index into get_hash_slice_starting_at_index where this chunk's range ends
                let end_index = std::cmp::min(start_index + num_hashes_per_chunk, total_hashes);

                // will compute the final result for this closure
                let mut hasher = Hasher::default();

                // index into 'data' where we are currently pulling data
                // if we exhaust our data, then we will request a new slice, and data_index resets to 0, the beginning of the new slice
                let mut data_index = start_index;
                // source data, which we may refresh when we exhaust
                let mut data = data;
                // len of the source data
                let mut data_len = data_len;

                if !three_level {
                    // 1 group of fanout
                    // The result of this loop is a single hash value from fanout input hashes.
                    for i in start_index..end_index {
                        if data_index >= data_len {
                            // we exhausted our data, fetch next slice starting at i
                            data = get_hash_slice_starting_at_index(i);
                            data_len = data.len();
                            data_index = 0;
                        }
                        hasher.hash(data[data_index].as_ref());
                        data_index += 1;
                    }
                } else {
                    // hash 3 levels of fanout simultaneously.
                    // This codepath produces 1 hash value for between 1..=fanout^3 input hashes.
                    // It is equivalent to running the normal merkle tree calculation 3 iterations on the input.
                    //
                    // big idea:
                    //  merkle trees usually reduce the input vector by a factor of fanout with each iteration
                    //  example with fanout 2:
                    //   start:     [0,1,2,3,4,5,6,7]      in our case: [...16M...] or really, 1B
                    //   iteration0 [.5, 2.5, 4.5, 6.5]                 [... 1M...]
                    //   iteration1 [1.5, 5.5]                          [...65k...]
                    //   iteration2 3.5                                 [...4k... ]
                    //  So iteration 0 consumes N elements, hashes them in groups of 'fanout' and produces a vector of N/fanout elements
                    //   and the process repeats until there is only 1 hash left.
                    //
                    //  With the three_level code path, we make each chunk we iterate of size fanout^3 (4096)
                    //  So, the input could be 16M hashes and the output will be 4k hashes, or N/fanout^3
                    //  The goal is to reduce the amount of data that has to be constructed and held in memory.
                    //  When we know we have enough hashes, then, in 1 pass, we hash 3 levels simultaneously, storing far fewer intermediate hashes.
                    //
                    // Now, some details:
                    // The result of this loop is a single hash value from fanout^3 input hashes.
                    // concepts:
                    //  what we're conceptually hashing: "raw_hashes"[start_index..end_index]
                    //   example: [a,b,c,d,e,f]
                    //   but... hashes[] may really be multiple vectors that are pieced together.
                    //   example: [[a,b],[c],[d,e,f]]
                    //   get_hash_slice_starting_at_index(any_index) abstracts that and returns a slice starting at raw_hashes[any_index..]
                    //   such that the end of get_hash_slice_starting_at_index may be <, >, or = end_index
                    //   example: get_hash_slice_starting_at_index(1) returns [b]
                    //            get_hash_slice_starting_at_index(3) returns [d,e,f]
                    // This code is basically 3 iterations of merkle tree hashing occurring simultaneously.
                    // The first fanout raw hashes are hashed in hasher_k. This is iteration0
                    // Once hasher_k has hashed fanout hashes, hasher_k's result hash is hashed in hasher_j and then discarded
                    // hasher_k then starts over fresh and hashes the next fanout raw hashes. This is iteration0 again for a new set of data.
                    // Once hasher_j has hashed fanout hashes (from k), hasher_j's result hash is hashed in hasher and then discarded
                    // Once hasher has hashed fanout hashes (from j), then the result of hasher is the hash for fanout^3 raw hashes.
                    // If there are < fanout^3 hashes, then this code stops when it runs out of raw hashes and returns whatever it hashed.
                    // This is always how the very last elements work in a merkle tree.
                    let mut i = start_index;
                    while i < end_index {
                        let mut hasher_j = Hasher::default();
                        for _j in 0..fanout {
                            let mut hasher_k = Hasher::default();
                            let end = std::cmp::min(end_index - i, fanout);
                            for _k in 0..end {
                                if data_index >= data_len {
                                    // we exhausted our data, fetch next slice starting at i
                                    data = get_hash_slice_starting_at_index(i);
                                    data_len = data.len();
                                    data_index = 0;
                                }
                                hasher_k.hash(data[data_index].as_ref());
                                data_index += 1;
                                i += 1;
                            }
                            hasher_j.hash(hasher_k.result().as_ref());
                            if i >= end_index {
                                break;
                            }
                        }
                        hasher.hash(hasher_j.result().as_ref());
                    }
                }

                hasher.result()
            })
            .collect();
        time.stop();
        debug!("hashing {} {}", total_hashes, time);

        if let Some(mut specific_level_count_value) = specific_level_count {
            specific_level_count_value -= levels_hashed;
            if specific_level_count_value == 0 {
                (Hash::default(), result)
            } else {
                assert!(specific_level_count_value > 0);
                // We did not hash the number of levels required by 'specific_level_count', so repeat
                Self::compute_merkle_root_from_slices_recurse(
                    result,
                    fanout,
                    max_levels_per_pass,
                    Some(specific_level_count_value),
                )
            }
        } else {
            (
                if result.len() == 1 {
                    result[0]
                } else {
                    Self::compute_merkle_root_recurse(result, fanout)
                },
                vec![], // no intermediate results needed by caller
            )
        }
    }

    pub fn compute_merkle_root_from_slices_recurse(
        hashes: Vec<Hash>,
        fanout: usize,
        max_levels_per_pass: Option<usize>,
        specific_level_count: Option<usize>,
    ) -> (Hash, Vec<Hash>) {
        Self::compute_merkle_root_from_slices(
            hashes.len(),
            fanout,
            max_levels_per_pass,
            |start| &hashes[start..],
            specific_level_count,
        )
    }

    pub fn accumulate_account_hashes(mut hashes: Vec<(Pubkey, Hash)>) -> Hash {
        Self::sort_hashes_by_pubkey(&mut hashes);

        Self::compute_merkle_root_loop(hashes, MERKLE_FANOUT, |i| i.1)
    }

    pub fn sort_hashes_by_pubkey(hashes: &mut Vec<(Pubkey, Hash)>) {
        hashes.par_sort_unstable_by(|a, b| a.0.cmp(&b.0));
    }

    fn flatten_hash_intermediate<T>(
        data_sections_by_pubkey: Vec<Vec<Vec<T>>>,
        stats: &mut HashStats,
    ) -> Vec<Vec<T>>
    where
        T: Clone,
    {
        // flatten this:
        // vec: just a level of hierarchy
        //   vec: 1 vec per PUBKEY_BINS_FOR_CALCULATING_HASHES
        //     vec: Intermediate data whose pubkey belongs in this division
        // into this:
        // vec: 1 vec per PUBKEY_BINS_FOR_CALCULATING_HASHES
        //   vec: Intermediate data whose pubkey belongs in this division
        let mut flatten_time = Measure::start("flatten");
        let mut data_by_pubkey: Vec<Vec<T>> = vec![];
        let mut raw_len = 0;
        for mut outer in data_sections_by_pubkey {
            let outer_len = outer.len();
            for pubkey_index in 0..outer_len {
                let this_len = outer[pubkey_index].len();
                if this_len == 0 {
                    continue;
                }
                raw_len += this_len;
                let mut data = vec![];
                std::mem::swap(&mut data, &mut outer[pubkey_index]);

                if data_by_pubkey.len() <= pubkey_index {
                    data_by_pubkey.extend(vec![vec![]; pubkey_index - data_by_pubkey.len() + 1]);
                }

                data_by_pubkey[pubkey_index].extend(data);
            }
        }
        flatten_time.stop();
        stats.flatten_time_total_us += flatten_time.as_us();
        stats.unreduced_entries += raw_len;
        data_by_pubkey
    }

    pub fn compare_two_hash_entries(
        a: &CalculateHashIntermediate,
        b: &CalculateHashIntermediate,
    ) -> std::cmp::Ordering {
        // note partial_cmp only returns None with floating point comparisons
        match a.pubkey.partial_cmp(&b.pubkey).unwrap() {
            std::cmp::Ordering::Equal => match b.slot.partial_cmp(&a.slot).unwrap() {
                std::cmp::Ordering::Equal => b.version.partial_cmp(&a.version).unwrap(),
                other => other,
            },
            other => other,
        }
    }

    fn sort_hash_intermediate(
        data_by_pubkey: Vec<Vec<CalculateHashIntermediate>>,
        stats: &mut HashStats,
    ) -> Vec<Vec<CalculateHashIntermediate>> {
        // sort each PUBKEY_DIVISION vec
        let mut sort_time = Measure::start("sort");
        let sorted_data_by_pubkey: Vec<Vec<_>> = data_by_pubkey
            .into_par_iter()
            .map(|mut pk_range| {
                pk_range.par_sort_unstable_by(Self::compare_two_hash_entries);
                pk_range
            })
            .collect();
        sort_time.stop();
        stats.sort_time_total_us += sort_time.as_us();
        sorted_data_by_pubkey
    }

    pub fn checked_cast_for_capitalization(balance: u128) -> u64 {
        balance
            .try_into()
            .expect("overflow is detected while summing capitalization")
    }

    fn de_dup_and_eliminate_zeros(
        sorted_data_by_pubkey: Vec<Vec<CalculateHashIntermediate>>,
        stats: &mut HashStats,
    ) -> (Vec<Vec<Vec<Hash>>>, u64) {
        // 1. eliminate zero lamport accounts
        // 2. pick the highest slot or (slot = and highest version) of each pubkey
        // 3. produce this output:
        // vec: PUBKEY_BINS_FOR_CALCULATING_HASHES in pubkey order
        //   vec: sorted sections from parallelism, in pubkey order
        //     vec: individual hashes in pubkey order
        let mut zeros = Measure::start("eliminate zeros");
        let overall_sum = Mutex::new(0u64);
        const CHUNKS: usize = 10;
        let hashes: Vec<Vec<Vec<Hash>>> = sorted_data_by_pubkey
            .into_par_iter()
            .map(|pubkey_division| {
                let (hashes, sum) = Self::de_dup_accounts_in_parallel(&pubkey_division, CHUNKS);
                let mut overall = overall_sum.lock().unwrap();
                *overall = Self::checked_cast_for_capitalization(sum as u128 + *overall as u128);
                hashes
            })
            .collect();
        zeros.stop();
        stats.zeros_time_total_us += zeros.as_us();
        let sum = *overall_sum.lock().unwrap();
        (hashes, sum)
    }

    // 1. eliminate zero lamport accounts
    // 2. pick the highest slot or (slot = and highest version) of each pubkey
    // 3. produce this output:
    //   vec: sorted sections from parallelism, in pubkey order
    //     vec: individual hashes in pubkey order
    fn de_dup_accounts_in_parallel(
        pubkey_division: &[CalculateHashIntermediate],
        chunk_count: usize,
    ) -> (Vec<Vec<Hash>>, u64) {
        let len = pubkey_division.len();
        let max = if len > chunk_count {
            std::cmp::max(chunk_count, 1)
        } else {
            1
        };
        let chunk_size = len / max;
        let overall_sum = Mutex::new(0u64);
        let hashes: Vec<Vec<Hash>> = (0..max)
            .into_par_iter()
            .map(|chunk_index| {
                let mut start_index = chunk_index * chunk_size;
                let mut end_index = start_index + chunk_size;
                if chunk_index == max - 1 {
                    end_index = len;
                }

                let is_first_slice = chunk_index == 0;
                if !is_first_slice {
                    // note that this causes all regions after region 0 to have 1 item that overlaps with the previous region
                    start_index -= 1;
                }

                let (result, sum) = Self::de_dup_accounts_from_stores(
                    chunk_index == 0,
                    &pubkey_division[start_index..end_index],
                );
                let mut overall = overall_sum.lock().unwrap();
                *overall = Self::checked_cast_for_capitalization(sum + *overall as u128);

                result
            })
            .collect();

        let sum = *overall_sum.lock().unwrap();
        (hashes, sum)
    }

    fn de_dup_accounts_from_stores(
        is_first_slice: bool,
        slice: &[CalculateHashIntermediate],
    ) -> (Vec<Hash>, u128) {
        let len = slice.len();
        let mut result: Vec<Hash> = Vec::with_capacity(len);

        let mut sum: u128 = 0;
        if len > 0 {
            let mut i = 0;
            // look_for_first_key means the first key we find in our slice may be a
            //  continuation of accounts belonging to a key that started in the last slice.
            // so, look_for_first_key=true means we have to find the first key different than
            //  the first key we encounter in our slice. Note that if this is true,
            //  our slice begins one index prior to the 'actual' start of our logical range.
            let mut look_for_first_key = !is_first_slice;
            'outer: loop {
                // at start of loop, item at 'i' is the first entry for a given pubkey - unless look_for_first
                let now = &slice[i];
                let last = now.pubkey;
                if !look_for_first_key && now.lamports != ZERO_RAW_LAMPORTS_SENTINEL {
                    // first entry for this key that starts in our slice
                    result.push(now.hash);
                    sum += now.lamports as u128;
                }
                for (k, now) in slice.iter().enumerate().skip(i + 1) {
                    if now.pubkey != last {
                        i = k;
                        look_for_first_key = false;
                        continue 'outer;
                    } else {
                        let prev = &slice[k - 1];
                        assert!(
                            !(prev.slot == now.slot
                                && prev.version == now.version
                                && (prev.hash != now.hash || prev.lamports != now.lamports)),
                            "Conflicting store data. Pubkey: {}, Slot: {}, Version: {}, Hashes: {}, {}, Lamports: {}, {}", now.pubkey, now.slot, now.version, prev.hash, now.hash, prev.lamports, now.lamports
                        );
                    }
                }

                break; // ran out of items in our slice, so our slice is done
            }
        }
        (result, sum)
    }

    // input:
    // vec: unordered, created by parallelism
    //   vec: [0..bins] - where bins are pubkey ranges
    //     vec: [..] - items which fit in the containing bin, unordered within this vec
    // so, assumption is middle vec is bins sorted by pubkey
    pub fn rest_of_hash_calculation(
        data_sections_by_pubkey: Vec<Vec<Vec<CalculateHashIntermediate>>>,
        mut stats: &mut HashStats,
        is_last_pass: bool,
        mut previous_state: PreviousPass,
    ) -> (Hash, u64, PreviousPass) {
        let outer = Self::flatten_hash_intermediate(data_sections_by_pubkey, &mut stats);

        let sorted_data_by_pubkey = Self::sort_hash_intermediate(outer, &mut stats);

        let (mut hashes, mut total_lamports) =
            Self::de_dup_and_eliminate_zeros(sorted_data_by_pubkey, &mut stats);

        total_lamports += previous_state.lamports;

        if !previous_state.remaining_unhashed.is_empty() {
            // these items were not hashed last iteration because they didn't divide evenly
            hashes.insert(0, vec![previous_state.remaining_unhashed]);
            previous_state.remaining_unhashed = Vec::new();
        }

        let mut next_pass = PreviousPass::default();
        let cumulative = CumulativeOffsets::from_raw_2d(&hashes);
        let mut hash_total = cumulative.total_count;
        stats.hash_total += hash_total;
        next_pass.reduced_hashes = previous_state.reduced_hashes;

        const TARGET_FANOUT_LEVEL: usize = 3;
        let target_fanout = MERKLE_FANOUT.pow(TARGET_FANOUT_LEVEL as u32);

        if !is_last_pass {
            next_pass.lamports = total_lamports;
            total_lamports = 0;

            // Save hashes that don't evenly hash. They will be combined with hashes from the next pass.
            let left_over_hashes = hash_total % target_fanout;

            // move tail hashes that don't evenly hash into a 1d vector for next time
            let mut i = hash_total - left_over_hashes;
            while i < hash_total {
                let data = cumulative.get_slice_2d(&hashes, i);
                next_pass.remaining_unhashed.extend(data);
                i += data.len();
            }

            hash_total -= left_over_hashes; // this is enough to cause the hashes at the end of the data set to be ignored
        }

        // if we have raw hashes to process and
        //   we are not the last pass (we already modded against target_fanout) OR
        //   we have previously surpassed target_fanout and hashed some already to the target_fanout level. In that case, we know
        //     we need to hash whatever is left here to the target_fanout level.
        if hash_total != 0 && (!is_last_pass || !next_pass.reduced_hashes.is_empty()) {
            let mut hash_time = Measure::start("hash");
            let partial_hashes = Self::compute_merkle_root_from_slices(
                hash_total, // note this does not include the ones that didn't divide evenly, unless we're in the last iteration
                MERKLE_FANOUT,
                Some(TARGET_FANOUT_LEVEL),
                |start| cumulative.get_slice_2d(&hashes, start),
                Some(TARGET_FANOUT_LEVEL),
            )
            .1;
            hash_time.stop();
            stats.hash_time_total_us += hash_time.as_us();
            next_pass.reduced_hashes.push(partial_hashes);
        }

        let no_progress = is_last_pass && next_pass.reduced_hashes.is_empty() && !hashes.is_empty();
        if no_progress {
            // we never made partial progress, so hash everything now
            hashes.into_iter().for_each(|v| {
                v.into_iter().for_each(|v| {
                    if !v.is_empty() {
                        next_pass.reduced_hashes.push(v);
                    }
                });
            });
        }

        let hash = if is_last_pass {
            let cumulative = CumulativeOffsets::from_raw(&next_pass.reduced_hashes);

            let hash = if cumulative.total_count == 1 && !no_progress {
                // all the passes resulted in a single hash, that means we're done, so we had <= MERKLE_ROOT total hashes
                cumulative.get_slice(&next_pass.reduced_hashes, 0)[0]
            } else {
                let mut hash_time = Measure::start("hash");
                // hash all the rest and combine and hash until we have only 1 hash left
                let (hash, _) = Self::compute_merkle_root_from_slices(
                    cumulative.total_count,
                    MERKLE_FANOUT,
                    None,
                    |start| cumulative.get_slice(&next_pass.reduced_hashes, start),
                    None,
                );
                hash_time.stop();
                stats.hash_time_total_us += hash_time.as_us();
                hash
            };
            next_pass.reduced_hashes = Vec::new();
            hash
        } else {
            Hash::default()
        };

        if is_last_pass {
            stats.log();
        }
        (hash, total_lamports, next_pass)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_accountsdb_div_ceil() {
        assert_eq!(AccountsHash::div_ceil(10, 3), 4);
        assert_eq!(AccountsHash::div_ceil(0, 1), 0);
        assert_eq!(AccountsHash::div_ceil(0, 5), 0);
        assert_eq!(AccountsHash::div_ceil(9, 3), 3);
        assert_eq!(AccountsHash::div_ceil(9, 9), 1);
    }

    #[test]
    #[should_panic(expected = "attempt to divide by zero")]
    fn test_accountsdb_div_ceil_fail() {
        assert_eq!(AccountsHash::div_ceil(10, 0), 0);
    }

    #[test]
    fn test_accountsdb_rest_of_hash_calculation() {
        solana_logger::setup();

        let mut account_maps: Vec<CalculateHashIntermediate> = Vec::new();

        let key = Pubkey::new(&[11u8; 32]);
        let hash = Hash::new(&[1u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 88, Slot::default(), key);
        account_maps.push(val);

        // 2nd key - zero lamports, so will be removed
        let key = Pubkey::new(&[12u8; 32]);
        let hash = Hash::new(&[2u8; 32]);
        let val = CalculateHashIntermediate::new(
            0,
            hash,
            ZERO_RAW_LAMPORTS_SENTINEL,
            Slot::default(),
            key,
        );
        account_maps.push(val);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![account_maps.clone()]],
            &mut HashStats::default(),
            true,
            PreviousPass::default(),
        );
        let expected_hash = Hash::from_str("8j9ARGFv4W2GfML7d3sVJK2MePwrikqYnu6yqer28cCa").unwrap();
        assert_eq!((result.0, result.1), (expected_hash, 88));

        // 3rd key - with pubkey value before 1st key so it will be sorted first
        let key = Pubkey::new(&[10u8; 32]);
        let hash = Hash::new(&[2u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 20, Slot::default(), key);
        account_maps.push(val);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![account_maps.clone()]],
            &mut HashStats::default(),
            true,
            PreviousPass::default(),
        );
        let expected_hash = Hash::from_str("EHv9C5vX7xQjjMpsJMzudnDTzoTSRwYkqLzY8tVMihGj").unwrap();
        assert_eq!((result.0, result.1), (expected_hash, 108));

        // 3rd key - with later slot
        let key = Pubkey::new(&[10u8; 32]);
        let hash = Hash::new(&[99u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 30, Slot::default() + 1, key);
        account_maps.push(val);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![account_maps]],
            &mut HashStats::default(),
            true,
            PreviousPass::default(),
        );
        let expected_hash = Hash::from_str("7NNPg5A8Xsg1uv4UFm6KZNwsipyyUnmgCrznP6MBWoBZ").unwrap();
        assert_eq!((result.0, result.1), (expected_hash, 118));
    }

    #[test]
    fn test_accountsdb_multi_pass_rest_of_hash_calculation() {
        solana_logger::setup();

        // passes:
        // 0: empty, NON-empty, empty, empty final
        // 1: NON-empty, empty final
        // 2: NON-empty, empty, empty final
        for pass in 0..3 {
            let mut account_maps: Vec<CalculateHashIntermediate> = Vec::new();

            let key = Pubkey::new(&[11u8; 32]);
            let hash = Hash::new(&[1u8; 32]);
            let val = CalculateHashIntermediate::new(0, hash, 88, Slot::default(), key);
            account_maps.push(val);

            // 2nd key - zero lamports, so will be removed
            let key = Pubkey::new(&[12u8; 32]);
            let hash = Hash::new(&[2u8; 32]);
            let val = CalculateHashIntermediate::new(
                0,
                hash,
                ZERO_RAW_LAMPORTS_SENTINEL,
                Slot::default(),
                key,
            );
            account_maps.push(val);

            let mut previous_pass = PreviousPass::default();

            if pass == 0 {
                // first pass that is not last and is empty
                let result = AccountsHash::rest_of_hash_calculation(
                    vec![vec![vec![]]],
                    &mut HashStats::default(),
                    false, // not last pass
                    previous_pass,
                );
                assert_eq!(result.0, Hash::default());
                assert_eq!(result.1, 0);
                previous_pass = result.2;
                assert_eq!(previous_pass.remaining_unhashed.len(), 0);
                assert_eq!(previous_pass.reduced_hashes.len(), 0);
                assert_eq!(previous_pass.lamports, 0);
            }

            let result = AccountsHash::rest_of_hash_calculation(
                vec![vec![account_maps.clone()]],
                &mut HashStats::default(),
                false, // not last pass
                previous_pass,
            );

            assert_eq!(result.0, Hash::default());
            assert_eq!(result.1, 0);
            let mut previous_pass = result.2;
            assert_eq!(previous_pass.remaining_unhashed, vec![account_maps[0].hash]);
            assert_eq!(previous_pass.reduced_hashes.len(), 0);
            assert_eq!(previous_pass.lamports, account_maps[0].lamports);

            let expected_hash =
                Hash::from_str("8j9ARGFv4W2GfML7d3sVJK2MePwrikqYnu6yqer28cCa").unwrap();
            if pass == 2 {
                let result = AccountsHash::rest_of_hash_calculation(
                    vec![vec![vec![]]],
                    &mut HashStats::default(),
                    false,
                    previous_pass,
                );

                previous_pass = result.2;
                assert_eq!(previous_pass.remaining_unhashed, vec![account_maps[0].hash]);
                assert_eq!(previous_pass.reduced_hashes.len(), 0);
                assert_eq!(previous_pass.lamports, account_maps[0].lamports);
            }

            let result = AccountsHash::rest_of_hash_calculation(
                vec![vec![vec![]]],
                &mut HashStats::default(),
                true, // finally, last pass
                previous_pass,
            );
            let previous_pass = result.2;

            assert_eq!(previous_pass.remaining_unhashed.len(), 0);
            assert_eq!(previous_pass.reduced_hashes.len(), 0);
            assert_eq!(previous_pass.lamports, 0);

            assert_eq!((result.0, result.1), (expected_hash, 88));
        }
    }

    #[test]
    fn test_accountsdb_multi_pass_rest_of_hash_calculation_partial() {
        solana_logger::setup();

        let mut account_maps: Vec<CalculateHashIntermediate> = Vec::new();

        let key = Pubkey::new(&[11u8; 32]);
        let hash = Hash::new(&[1u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 88, Slot::default(), key);
        account_maps.push(val);

        let key = Pubkey::new(&[12u8; 32]);
        let hash = Hash::new(&[2u8; 32]);
        let val = CalculateHashIntermediate::new(0, hash, 20, Slot::default(), key);
        account_maps.push(val);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![vec![account_maps[0].clone()]]],
            &mut HashStats::default(),
            false, // not last pass
            PreviousPass::default(),
        );

        assert_eq!(result.0, Hash::default());
        assert_eq!(result.1, 0);
        let previous_pass = result.2;
        assert_eq!(previous_pass.remaining_unhashed, vec![account_maps[0].hash]);
        assert_eq!(previous_pass.reduced_hashes.len(), 0);
        assert_eq!(previous_pass.lamports, account_maps[0].lamports);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![vec![account_maps[1].clone()]]],
            &mut HashStats::default(),
            false, // not last pass
            previous_pass,
        );

        assert_eq!(result.0, Hash::default());
        assert_eq!(result.1, 0);
        let previous_pass = result.2;
        assert_eq!(
            previous_pass.remaining_unhashed,
            vec![account_maps[0].hash, account_maps[1].hash]
        );
        assert_eq!(previous_pass.reduced_hashes.len(), 0);
        let total_lamports_expected = account_maps[0].lamports + account_maps[1].lamports;
        assert_eq!(previous_pass.lamports, total_lamports_expected);

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![vec![]]],
            &mut HashStats::default(),
            true,
            previous_pass,
        );

        let previous_pass = result.2;
        assert_eq!(previous_pass.remaining_unhashed.len(), 0);
        assert_eq!(previous_pass.reduced_hashes.len(), 0);
        assert_eq!(previous_pass.lamports, 0);

        let expected_hash = AccountsHash::compute_merkle_root(
            account_maps
                .iter()
                .map(|a| (a.pubkey, a.hash))
                .collect::<Vec<_>>(),
            MERKLE_FANOUT,
        );

        assert_eq!(
            (result.0, result.1),
            (expected_hash, total_lamports_expected)
        );
    }

    #[test]
    fn test_accountsdb_multi_pass_rest_of_hash_calculation_partial_hashes() {
        solana_logger::setup();

        let mut account_maps: Vec<CalculateHashIntermediate> = Vec::new();

        const TARGET_FANOUT_LEVEL: usize = 3;
        let target_fanout = MERKLE_FANOUT.pow(TARGET_FANOUT_LEVEL as u32);
        let mut total_lamports_expected = 0;
        let plus1 = target_fanout + 1;
        for i in 0..plus1 * 2 {
            let lamports = (i + 1) as u64;
            total_lamports_expected += lamports;
            let key = Pubkey::new_unique();
            let hash = Hash::new_unique();
            let val = CalculateHashIntermediate::new(0, hash, lamports, Slot::default(), key);
            account_maps.push(val);
        }

        let chunk = account_maps[0..plus1].to_vec();
        let mut sorted = chunk.clone();
        sorted.sort_by(AccountsHash::compare_two_hash_entries);

        // first 4097 hashes (1 left over)
        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![chunk]],
            &mut HashStats::default(),
            false, // not last pass
            PreviousPass::default(),
        );

        assert_eq!(result.0, Hash::default());
        assert_eq!(result.1, 0);
        let previous_pass = result.2;
        let left_over_1 = sorted[plus1 - 1].hash;
        assert_eq!(previous_pass.remaining_unhashed, vec![left_over_1]);
        assert_eq!(previous_pass.reduced_hashes.len(), 1);
        let expected_hash = AccountsHash::compute_merkle_root(
            sorted[0..target_fanout]
                .iter()
                .map(|a| (a.pubkey, a.hash))
                .collect::<Vec<_>>(),
            MERKLE_FANOUT,
        );
        assert_eq!(previous_pass.reduced_hashes[0], vec![expected_hash]);
        assert_eq!(
            previous_pass.lamports,
            account_maps[0..plus1]
                .iter()
                .map(|i| i.lamports)
                .sum::<u64>()
        );

        let chunk = account_maps[plus1..plus1 * 2].to_vec();
        let mut sorted2 = chunk.clone();
        sorted2.sort_by(AccountsHash::compare_two_hash_entries);

        let mut with_left_over = vec![left_over_1];
        with_left_over.extend(sorted2[0..plus1 - 2].to_vec().into_iter().map(|i| i.hash));
        let expected_hash2 = AccountsHash::compute_merkle_root(
            with_left_over[0..target_fanout]
                .iter()
                .map(|a| (Pubkey::default(), *a))
                .collect::<Vec<_>>(),
            MERKLE_FANOUT,
        );

        // second 4097 hashes (2 left over)
        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![chunk]],
            &mut HashStats::default(),
            false, // not last pass
            previous_pass,
        );

        assert_eq!(result.0, Hash::default());
        assert_eq!(result.1, 0);
        let previous_pass = result.2;
        assert_eq!(
            previous_pass.remaining_unhashed,
            vec![sorted2[plus1 - 2].hash, sorted2[plus1 - 1].hash]
        );
        assert_eq!(previous_pass.reduced_hashes.len(), 2);
        assert_eq!(
            previous_pass.reduced_hashes,
            vec![vec![expected_hash], vec![expected_hash2]]
        );
        assert_eq!(
            previous_pass.lamports,
            account_maps[0..plus1 * 2]
                .iter()
                .map(|i| i.lamports)
                .sum::<u64>()
        );

        let result = AccountsHash::rest_of_hash_calculation(
            vec![vec![vec![]]],
            &mut HashStats::default(),
            true,
            previous_pass,
        );

        let previous_pass = result.2;
        assert_eq!(previous_pass.remaining_unhashed.len(), 0);
        assert_eq!(previous_pass.reduced_hashes.len(), 0);
        assert_eq!(previous_pass.lamports, 0);

        let mut combined = sorted;
        combined.extend(sorted2);
        let expected_hash = AccountsHash::compute_merkle_root(
            combined
                .iter()
                .map(|a| (a.pubkey, a.hash))
                .collect::<Vec<_>>(),
            MERKLE_FANOUT,
        );

        assert_eq!(
            (result.0, result.1),
            (expected_hash, total_lamports_expected)
        );
    }

    #[test]
    fn test_accountsdb_de_dup_accounts_zero_chunks() {
        let (hashes, lamports) =
            AccountsHash::de_dup_accounts_in_parallel(&[CalculateHashIntermediate::default()], 0);
        assert_eq!(vec![vec![Hash::default()]], hashes);
        assert_eq!(lamports, 0);
    }

    #[test]
    fn test_accountsdb_de_dup_accounts_empty() {
        solana_logger::setup();

        let (hashes, lamports) = AccountsHash::de_dup_and_eliminate_zeros(
            vec![vec![], vec![]],
            &mut HashStats::default(),
        );
        assert_eq!(
            vec![vec![Hash::default(); 0], vec![]],
            hashes.into_iter().flatten().collect::<Vec<_>>()
        );
        assert_eq!(lamports, 0);

        let (hashes, lamports) =
            AccountsHash::de_dup_and_eliminate_zeros(vec![], &mut HashStats::default());
        let empty: Vec<Vec<Vec<Hash>>> = Vec::default();
        assert_eq!(empty, hashes);
        assert_eq!(lamports, 0);

        let (hashes, lamports) = AccountsHash::de_dup_accounts_in_parallel(&[], 1);
        assert_eq!(
            vec![Hash::default(); 0],
            hashes.into_iter().flatten().collect::<Vec<_>>()
        );
        assert_eq!(lamports, 0);

        let (hashes, lamports) = AccountsHash::de_dup_accounts_in_parallel(&[], 2);
        assert_eq!(
            vec![Hash::default(); 0],
            hashes.into_iter().flatten().collect::<Vec<_>>()
        );
        assert_eq!(lamports, 0);
    }

    #[test]
    fn test_accountsdb_de_dup_accounts_from_stores() {
        solana_logger::setup();

        let key_a = Pubkey::new(&[1u8; 32]);
        let key_b = Pubkey::new(&[2u8; 32]);
        let key_c = Pubkey::new(&[3u8; 32]);
        const COUNT: usize = 6;
        const VERSION: u64 = 0;
        let hashes: Vec<_> = (0..COUNT)
            .into_iter()
            .map(|i| Hash::new(&[i as u8; 32]))
            .collect();
        // create this vector
        // abbbcc
        let keys = [key_a, key_b, key_b, key_b, key_c, key_c];

        let accounts: Vec<_> = hashes
            .into_iter()
            .zip(keys.iter())
            .enumerate()
            .map(|(i, (hash, key))| {
                CalculateHashIntermediate::new(
                    VERSION,
                    hash,
                    (i + 1) as u64,
                    u64::MAX - i as u64,
                    *key,
                )
            })
            .collect();

        type ExpectedType = (String, bool, u64, String);
        let expected:Vec<ExpectedType> = vec![
            // ("key/lamports key2/lamports ...",
            // is_first_slice
            // result lamports
            // result hashes)
            // "a5" = key_a, 5 lamports
            ("a1", false, 0, "[]"),
            ("a1b2", false, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3", false, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3b4", false, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3b4c5", false, 7, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi, GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b2", false, 0, "[]"),
            ("b2b3", false, 0, "[]"),
            ("b2b3b4", false, 0, "[]"),
            ("b2b3b4c5", false, 5, "[GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b3", false, 0, "[]"),
            ("b3b4", false, 0, "[]"),
            ("b3b4c5", false, 5, "[GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b4", false, 0, "[]"),
            ("b4c5", false, 5, "[GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("c5", false, 0, "[]"),
            ("a1", true, 1, "[11111111111111111111111111111111]"),
            ("a1b2", true, 3, "[11111111111111111111111111111111, 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3", true, 3, "[11111111111111111111111111111111, 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3b4", true, 3, "[11111111111111111111111111111111, 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("a1b2b3b4c5", true, 8, "[11111111111111111111111111111111, 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi, GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b2", true, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("b2b3", true, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("b2b3b4", true, 2, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi]"),
            ("b2b3b4c5", true, 7, "[4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi, GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b3", true, 3, "[8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR]"),
            ("b3b4", true, 3, "[8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR]"),
            ("b3b4c5", true, 8, "[8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR, GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("b4", true, 4, "[CktRuQ2mttgRGkXJtyksdKHjUdc2C4TgDzyB98oEzy8]"),
            ("b4c5", true, 9, "[CktRuQ2mttgRGkXJtyksdKHjUdc2C4TgDzyB98oEzy8, GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ("c5", true, 5, "[GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq]"),
            ].into_iter().map(|item| {
                let result: ExpectedType = (
                    item.0.to_string(),
                    item.1,
                    item.2,
                    item.3.to_string(),
                );
                result
            }).collect();

        let mut expected_index = 0;
        for first_slice in 0..2 {
            for start in 0..COUNT {
                for end in start + 1..COUNT {
                    let is_first_slice = first_slice == 1;
                    let accounts = accounts.clone();
                    let slice = &accounts[start..end];

                    let result = AccountsHash::de_dup_accounts_from_stores(is_first_slice, slice);
                    let (hashes2, lamports2) = AccountsHash::de_dup_accounts_in_parallel(slice, 1);
                    let (hashes3, lamports3) = AccountsHash::de_dup_accounts_in_parallel(slice, 2);
                    let (hashes4, lamports4) = AccountsHash::de_dup_and_eliminate_zeros(
                        vec![slice.to_vec()],
                        &mut HashStats::default(),
                    );
                    let (hashes5, lamports5) = AccountsHash::de_dup_and_eliminate_zeros(
                        vec![slice.to_vec(), slice.to_vec()],
                        &mut HashStats::default(),
                    );
                    let (hashes6, lamports6) = AccountsHash::de_dup_and_eliminate_zeros(
                        vec![vec![], slice.to_vec()],
                        &mut HashStats::default(),
                    );

                    assert_eq!(
                        hashes2.iter().flatten().collect::<Vec<_>>(),
                        hashes3.iter().flatten().collect::<Vec<_>>()
                    );
                    let expected2 = hashes2.clone().into_iter().flatten().collect::<Vec<_>>();
                    assert_eq!(
                        expected2,
                        hashes4
                            .into_iter()
                            .flatten()
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        vec![expected2.clone(), expected2.clone()],
                        hashes5.into_iter().flatten().collect::<Vec<_>>()
                    );
                    assert_eq!(
                        vec![vec![], expected2.clone()],
                        hashes6.into_iter().flatten().collect::<Vec<_>>()
                    );
                    assert_eq!(lamports2, lamports3);
                    assert_eq!(lamports2, lamports4);
                    assert_eq!(lamports2 * 2, lamports5);
                    assert_eq!(lamports2, lamports6);

                    let hashes: Vec<_> = hashes2.into_iter().flatten().collect();

                    let human_readable = slice
                        .iter()
                        .map(|v| {
                            let mut s = (if v.pubkey == key_a {
                                "a"
                            } else if v.pubkey == key_b {
                                "b"
                            } else {
                                "c"
                            })
                            .to_string();

                            s.push_str(&v.lamports.to_string());
                            s
                        })
                        .collect::<String>();

                    let hash_result_as_string = format!("{:?}", result.0);

                    let packaged_result: ExpectedType = (
                        human_readable,
                        is_first_slice,
                        result.1 as u64,
                        hash_result_as_string,
                    );

                    if is_first_slice {
                        // the parallel version always starts with 'first slice'
                        assert_eq!(
                            result.0, hashes,
                            "description: {:?}, expected index: {}",
                            packaged_result, expected_index
                        );
                        assert_eq!(
                            result.1 as u64, lamports2,
                            "description: {:?}, expected index: {}",
                            packaged_result, expected_index
                        );
                    }

                    assert_eq!(expected[expected_index], packaged_result);

                    // for generating expected results
                    // error!("{:?},", packaged_result);
                    expected_index += 1;
                }
            }
        }

        for first_slice in 0..2 {
            let result = AccountsHash::de_dup_accounts_from_stores(first_slice == 1, &[]);
            assert_eq!((vec![Hash::default(); 0], 0), result);
        }
    }

    #[test]
    fn test_sort_hash_intermediate() {
        solana_logger::setup();
        let mut stats = HashStats::default();
        let key = Pubkey::new_unique();
        let hash = Hash::new_unique();
        let val = CalculateHashIntermediate::new(1, hash, 1, 1, key);

        // slot same, version <
        let hash2 = Hash::new_unique();
        let val2 = CalculateHashIntermediate::new(0, hash2, 4, 1, key);
        let val3 = CalculateHashIntermediate::new(3, hash2, 4, 1, key);
        let val4 = CalculateHashIntermediate::new(4, hash2, 4, 1, key);

        let src = vec![vec![val2.clone()], vec![val.clone()]];
        let result = AccountsHash::sort_hash_intermediate(src.clone(), &mut stats);
        assert_eq!(result, src);

        let src = vec![
            vec![val2.clone(), val.clone()],
            vec![val3.clone(), val4.clone()],
        ];
        let sorted = vec![vec![val, val2], vec![val4, val3]];
        let result = AccountsHash::sort_hash_intermediate(src, &mut stats);
        assert_eq!(result, sorted);

        let src = vec![vec![]];
        let result = AccountsHash::sort_hash_intermediate(src.clone(), &mut stats);
        assert_eq!(result, src);

        let src = vec![];
        let result = AccountsHash::sort_hash_intermediate(src.clone(), &mut stats);
        assert_eq!(result, src);
    }

    #[test]
    fn test_accountsdb_compare_two_hash_entries() {
        solana_logger::setup();
        let key = Pubkey::new_unique();
        let hash = Hash::new_unique();
        let val = CalculateHashIntermediate::new(1, hash, 1, 1, key);

        // slot same, version <
        let hash2 = Hash::new_unique();
        let val2 = CalculateHashIntermediate::new(0, hash2, 4, 1, key);
        assert_eq!(
            std::cmp::Ordering::Less,
            AccountsHash::compare_two_hash_entries(&val, &val2)
        );

        let list = vec![val.clone(), val2.clone()];
        let mut list_bkup = list.clone();
        list_bkup.sort_by(AccountsHash::compare_two_hash_entries);
        let list = AccountsHash::sort_hash_intermediate(vec![list], &mut HashStats::default());
        assert_eq!(list, vec![list_bkup]);

        let list = vec![val2, val.clone()]; // reverse args
        let mut list_bkup = list.clone();
        list_bkup.sort_by(AccountsHash::compare_two_hash_entries);
        let list = AccountsHash::sort_hash_intermediate(vec![list], &mut HashStats::default());
        assert_eq!(list, vec![list_bkup]);

        // slot same, vers =
        let hash3 = Hash::new_unique();
        let val3 = CalculateHashIntermediate::new(1, hash3, 2, 1, key);
        assert_eq!(
            std::cmp::Ordering::Equal,
            AccountsHash::compare_two_hash_entries(&val, &val3)
        );

        // slot same, vers >
        let hash4 = Hash::new_unique();
        let val4 = CalculateHashIntermediate::new(2, hash4, 6, 1, key);
        assert_eq!(
            std::cmp::Ordering::Greater,
            AccountsHash::compare_two_hash_entries(&val, &val4)
        );

        // slot >, version <
        let hash5 = Hash::new_unique();
        let val5 = CalculateHashIntermediate::new(0, hash5, 8, 2, key);
        assert_eq!(
            std::cmp::Ordering::Greater,
            AccountsHash::compare_two_hash_entries(&val, &val5)
        );
    }

    #[test]
    fn test_accountsdb_remove_zero_balance_accounts() {
        solana_logger::setup();

        let key = Pubkey::new_unique();
        let hash = Hash::new_unique();
        let mut account_maps: Vec<CalculateHashIntermediate> = Vec::new();
        let val = CalculateHashIntermediate::new(0, hash, 1, Slot::default(), key);
        account_maps.push(val.clone());

        let result = AccountsHash::de_dup_accounts_from_stores(true, &account_maps[..]);
        assert_eq!(result, (vec![val.hash], val.lamports as u128));

        // zero original lamports, higher version
        let val = CalculateHashIntermediate::new(
            1,
            hash,
            ZERO_RAW_LAMPORTS_SENTINEL,
            Slot::default(),
            key,
        );
        account_maps.insert(0, val); // has to be before other entry since sort order matters

        let result = AccountsHash::de_dup_accounts_from_stores(true, &account_maps[..]);
        assert_eq!(result, (vec![], 0));
    }

    #[test]
    fn test_accountsdb_cumulative_offsets1_d() {
        let input = vec![vec![0, 1], vec![], vec![2, 3, 4], vec![]];
        let cumulative = CumulativeOffsets::from_raw(&input);

        let src: Vec<_> = input.clone().into_iter().flatten().collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 2); // 2 non-empty vectors

        const DIMENSION: usize = 0;
        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION], 0);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION], 2);

        assert_eq!(cumulative.cumulative_offsets[0].start_offset, 0);
        assert_eq!(cumulative.cumulative_offsets[1].start_offset, 2);

        for start in 0..len {
            let slice = cumulative.get_slice(&input, start);
            let len = slice.len();
            assert!(len > 0);
            assert_eq!(&src[start..(start + len)], slice);
        }

        let input = vec![vec![], vec![0, 1], vec![], vec![2, 3, 4], vec![]];
        let cumulative = CumulativeOffsets::from_raw(&input);

        let src: Vec<_> = input.clone().into_iter().flatten().collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 2); // 2 non-empty vectors

        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION], 1);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION], 3);

        assert_eq!(cumulative.cumulative_offsets[0].start_offset, 0);
        assert_eq!(cumulative.cumulative_offsets[1].start_offset, 2);

        for start in 0..len {
            let slice = cumulative.get_slice(&input, start);
            let len = slice.len();
            assert!(len > 0);
            assert_eq!(&src[start..(start + len)], slice);
        }

        let input: Vec<Vec<u32>> = vec![vec![]];
        let cumulative = CumulativeOffsets::from_raw(&input);

        let src: Vec<_> = input.into_iter().flatten().collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 0); // 2 non-empty vectors
    }

    #[test]
    fn test_accountsdb_cumulative_offsets2_d() {
        let input = vec![vec![vec![0, 1], vec![], vec![2, 3, 4], vec![]]];
        let cumulative = CumulativeOffsets::from_raw_2d(&input);

        let src: Vec<_> = input
            .clone()
            .into_iter()
            .flatten()
            .into_iter()
            .flatten()
            .collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 2); // 2 non-empty vectors

        const DIMENSION_0: usize = 0;
        const DIMENSION_1: usize = 1;
        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_0], 0);
        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_1], 0);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_0], 0);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_1], 2);

        assert_eq!(cumulative.cumulative_offsets[0].start_offset, 0);
        assert_eq!(cumulative.cumulative_offsets[1].start_offset, 2);

        for start in 0..len {
            let slice = cumulative.get_slice_2d(&input, start);
            let len = slice.len();
            assert!(len > 0);
            assert_eq!(&src[start..(start + len)], slice);
        }

        let input = vec![vec![vec![], vec![0, 1], vec![], vec![2, 3, 4], vec![]]];
        let cumulative = CumulativeOffsets::from_raw_2d(&input);

        let src: Vec<_> = input
            .clone()
            .into_iter()
            .flatten()
            .into_iter()
            .flatten()
            .collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 2); // 2 non-empty vectors

        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_0], 0);
        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_1], 1);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_0], 0);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_1], 3);

        assert_eq!(cumulative.cumulative_offsets[0].start_offset, 0);
        assert_eq!(cumulative.cumulative_offsets[1].start_offset, 2);

        for start in 0..len {
            let slice = cumulative.get_slice_2d(&input, start);
            let len = slice.len();
            assert!(len > 0);
            assert_eq!(&src[start..(start + len)], slice);
        }

        let input: Vec<Vec<Vec<u32>>> = vec![vec![]];
        let cumulative = CumulativeOffsets::from_raw_2d(&input);

        let src: Vec<_> = input.into_iter().flatten().collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 0); // 2 non-empty vectors

        let input = vec![
            vec![vec![0, 1]],
            vec![vec![]],
            vec![vec![], vec![2, 3, 4], vec![]],
        ];
        let cumulative = CumulativeOffsets::from_raw_2d(&input);

        let src: Vec<_> = input
            .clone()
            .into_iter()
            .flatten()
            .into_iter()
            .flatten()
            .collect();
        let len = src.len();
        assert_eq!(cumulative.total_count, len);
        assert_eq!(cumulative.cumulative_offsets.len(), 2); // 2 non-empty vectors

        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_0], 0);
        assert_eq!(cumulative.cumulative_offsets[0].index[DIMENSION_1], 0);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_0], 2);
        assert_eq!(cumulative.cumulative_offsets[1].index[DIMENSION_1], 1);

        assert_eq!(cumulative.cumulative_offsets[0].start_offset, 0);
        assert_eq!(cumulative.cumulative_offsets[1].start_offset, 2);

        for start in 0..len {
            let slice = cumulative.get_slice_2d(&input, start);
            let len = slice.len();
            assert!(len > 0);
            assert_eq!(&src[start..(start + len)], slice);
        }
    }

    #[test]
    fn test_accountsdb_flatten_hash_intermediate() {
        solana_logger::setup();
        let test = vec![vec![vec![CalculateHashIntermediate::new(
            1,
            Hash::new_unique(),
            2,
            3,
            Pubkey::new_unique(),
        )]]];
        let mut stats = HashStats::default();
        let result = AccountsHash::flatten_hash_intermediate(test.clone(), &mut stats);
        assert_eq!(result, test[0]);
        assert_eq!(stats.unreduced_entries, 1);

        let mut stats = HashStats::default();
        let result = AccountsHash::flatten_hash_intermediate(
            vec![vec![vec![CalculateHashIntermediate::default(); 0]]],
            &mut stats,
        );
        assert_eq!(result.iter().flatten().count(), 0);
        assert_eq!(stats.unreduced_entries, 0);

        let test = vec![
            vec![vec![
                CalculateHashIntermediate::new(1, Hash::new_unique(), 2, 3, Pubkey::new_unique()),
                CalculateHashIntermediate::new(8, Hash::new_unique(), 9, 10, Pubkey::new_unique()),
            ]],
            vec![vec![CalculateHashIntermediate::new(
                4,
                Hash::new_unique(),
                5,
                6,
                Pubkey::new_unique(),
            )]],
        ];
        let mut stats = HashStats::default();
        let result = AccountsHash::flatten_hash_intermediate(test.clone(), &mut stats);
        let expected = test
            .into_iter()
            .flatten()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(result.into_iter().flatten().collect::<Vec<_>>(), expected);
        assert_eq!(stats.unreduced_entries, expected.len());
    }

    #[test]
    fn test_accountsdb_flatten_hash_intermediate2() {
        solana_logger::setup();
        // data is ordered:
        // vec: just a level of hierarchy
        //   vec: 1 vec per PUBKEY_BINS_FOR_CALCULATING_HASHES
        //     vec: Intermediate data whose pubkey belongs in this division
        let binned_data = vec![
            vec![vec![1, 2], vec![3, 4], vec![], vec![5]],
            vec![vec![], vec![11, 12]],
        ];
        let mut combined: Vec<Vec<Vec<i32>>> = vec![vec![]];
        binned_data.iter().enumerate().for_each(|(bin, v)| {
            v.iter()
                .enumerate()
                .for_each(|(dimension0, v): (usize, &Vec<i32>)| {
                    while combined.len() <= dimension0 {
                        combined.push(vec![]);
                    }
                    let vec: &mut Vec<Vec<i32>> = &mut combined[dimension0];
                    while vec.len() <= bin {
                        vec.push(vec![]);
                    }
                    vec[bin].extend(v.clone());
                });
        });

        let mut stats = HashStats::default();
        let result = AccountsHash::flatten_hash_intermediate(combined, &mut stats);
        assert_eq!(
            result,
            binned_data
                .clone()
                .into_iter()
                .map(|x| x.into_iter().flatten().collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            stats.unreduced_entries,
            binned_data
                .into_iter()
                .flatten()
                .into_iter()
                .flatten()
                .count()
        );

        let src = vec![vec![vec![0]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        assert_eq!(result, vec![vec![0]]);

        let src = vec![vec![vec![0], vec![1]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        assert_eq!(result, vec![vec![0], vec![1]]);

        let src = vec![vec![vec![]], vec![vec![], vec![1]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        assert_eq!(result, vec![vec![], vec![1]]);

        let src: Vec<Vec<Vec<i32>>> = vec![vec![vec![], vec![]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        let expected: Vec<Vec<i32>> = vec![];
        assert_eq!(result, expected);

        let src: Vec<Vec<Vec<i32>>> = vec![vec![vec![], vec![]], vec![vec![], vec![]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        assert_eq!(result, expected);

        let src: Vec<Vec<Vec<i32>>> = vec![vec![vec![], vec![]], vec![vec![]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        assert_eq!(result, expected);

        let src: Vec<Vec<Vec<i32>>> = vec![vec![], vec![vec![]]];
        let result = AccountsHash::flatten_hash_intermediate(src, &mut stats);
        let expected: Vec<Vec<i32>> = vec![];
        assert_eq!(result, expected);
    }

    fn test_hashing_larger(hashes: Vec<(Pubkey, Hash)>, fanout: usize) -> Hash {
        let result = AccountsHash::compute_merkle_root(hashes.clone(), fanout);
        let reduced: Vec<_> = hashes.iter().map(|x| x.1).collect();
        let result2 = test_hashing(reduced, fanout);
        assert_eq!(result, result2, "len: {}", hashes.len());
        result
    }

    fn test_hashing(hashes: Vec<Hash>, fanout: usize) -> Hash {
        let temp: Vec<_> = hashes.iter().map(|h| (Pubkey::default(), *h)).collect();
        let result = AccountsHash::compute_merkle_root(temp, fanout);
        let reduced: Vec<_> = hashes.clone();
        let result2 = AccountsHash::compute_merkle_root_from_slices(
            hashes.len(),
            fanout,
            None,
            |start| &reduced[start..],
            None,
        );
        assert_eq!(result, result2.0, "len: {}", hashes.len());

        let result2 = AccountsHash::compute_merkle_root_from_slices(
            hashes.len(),
            fanout,
            Some(1),
            |start| &reduced[start..],
            None,
        );
        assert_eq!(result, result2.0, "len: {}", hashes.len());

        let max = std::cmp::min(reduced.len(), fanout * 2);
        for left in 0..max {
            for right in left + 1..max {
                let src = vec![
                    vec![reduced[0..left].to_vec(), reduced[left..right].to_vec()],
                    vec![reduced[right..].to_vec()],
                ];
                let offsets = CumulativeOffsets::from_raw_2d(&src);

                let get_slice = |start: usize| -> &[Hash] { offsets.get_slice_2d(&src, start) };
                let result2 = AccountsHash::compute_merkle_root_from_slices(
                    offsets.total_count,
                    fanout,
                    None,
                    get_slice,
                    None,
                );
                assert_eq!(result, result2.0);
            }
        }
        result
    }

    #[test]
    fn test_accountsdb_compute_merkle_root_large() {
        solana_logger::setup();

        // handle fanout^x -1, +0, +1 for a few 'x's
        const FANOUT: usize = 3;
        let mut hash_counts: Vec<_> = (1..6)
            .flat_map(|x| {
                let mark = FANOUT.pow(x);
                vec![mark - 1, mark, mark + 1]
            })
            .collect();

        // saturate the test space for threshold to threshold + target
        // this hits right before we use the 3 deep optimization and all the way through all possible partial last chunks
        let target = FANOUT.pow(3);
        let threshold = target * FANOUT;
        hash_counts.extend(threshold - 1..=threshold + target);

        for hash_count in hash_counts {
            let hashes: Vec<_> = (0..hash_count)
                .into_iter()
                .map(|_| Hash::new_unique())
                .collect();

            test_hashing(hashes, FANOUT);
        }
    }

    #[test]
    fn test_accountsdb_compute_merkle_root() {
        solana_logger::setup();

        let expected_results = vec![
            (0, 0, "GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn", 0),
            (0, 1, "8unXKJYTxrR423HgQxbDmx29mFri1QNrzVKKDxEfc6bj", 0),
            (0, 2, "6QfkevXLLqbfAaR1kVjvMLFtEXvNUVrpmkwXqgsYtCFW", 1),
            (0, 3, "G3FrJd9JrXcMiqChTSfvEdBL2sCPny3ebiUy9Xxbn7a2", 3),
            (0, 4, "G3sZXHhwoCFuNyWy7Efffr47RBW33ibEp7b2hqNDmXdu", 6),
            (0, 5, "78atJJYpokAPKMJwHxUW8SBDvPkkSpTBV7GiB27HwosJ", 10),
            (0, 6, "7c9SM2BmCRVVXdrEdKcMK91MviPqXqQMd8QAb77tgLEy", 15),
            (0, 7, "3hsmnZPhf22UvBLiZ4dVa21Qsdh65CCrtYXsb8MxoVAa", 21),
            (0, 8, "5bwXUiC6RCRhb8fqvjvUXT6waU25str3UXA3a6Aq1jux", 28),
            (0, 9, "3NNtQKH6PaYpCnFBtyi2icK9eYX3YM5pqA3SKaXtUNzu", 36),
            (1, 0, "GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn", 0),
            (1, 1, "4GWVCsnEu1iRyxjAB3F7J7C4MMvcoxFWtP9ihvwvDgxY", 0),
            (1, 2, "8ML8Te6Uw2mipFr2v9sMZDcziXzhVqJo2qeMJohg1CJx", 1),
            (1, 3, "AMEuC3AgqAeRBGBhSfTmuMdfbAiXJnGmKv99kHmcAE1H", 3),
            (1, 4, "HEnDuJLHpsQfrApimGrovTqPEF6Vkrx2dKFr3BDtYzWx", 6),
            (1, 5, "6rH69iP2yM1o565noZN1EqjySW4PhYUskz3c5tXePUfV", 10),
            (1, 6, "7qEQMEXdfSPjbZ3q4cuuZwebDMvTvuaQ3dBiHoDUKo9a", 15),
            (1, 7, "GDJz7LSKYjqqz6ujCaaQRJRmQ7TLNCwYJhdT84qT4qwk", 21),
            (1, 8, "HT9krPLVTo3rr5WZQBQFrbqWs8SbYScXfnt8EVuobboM", 28),
            (1, 9, "8y2pMgqMdRsvqw6BQXm6wtz3qxGPss72i6H6gVpPyeda", 36),
        ];

        let mut expected_index = 0;
        let start = 0;
        let default_fanout = 2;
        // test 0..3 recursions (at fanout = 2) and 1 item remainder. The internals have 1 special case first loop and subsequent loops are the same types.
        let iterations = default_fanout * default_fanout * default_fanout + 2;
        for pass in 0..2 {
            let fanout = if pass == 0 {
                default_fanout
            } else {
                MERKLE_FANOUT
            };
            for count in start..iterations {
                let mut input: Vec<_> = (0..count)
                    .map(|i| {
                        let key = Pubkey::new(&[(pass * iterations + count) as u8; 32]);
                        let hash = Hash::new(&[(pass * iterations + count + i + 1) as u8; 32]);
                        (key, hash)
                    })
                    .collect();

                let result = if pass == 0 {
                    test_hashing_larger(input.clone(), fanout)
                } else {
                    // this sorts inside
                    let early_result = AccountsHash::accumulate_account_hashes(
                        input.iter().map(|i| (i.0, i.1)).collect::<Vec<_>>(),
                    );
                    AccountsHash::sort_hashes_by_pubkey(&mut input);
                    let result = AccountsHash::compute_merkle_root(input.clone(), fanout);
                    assert_eq!(early_result, result);
                    result
                };
                // compare against captured, expected results for hash (and lamports)
                assert_eq!(
                    (
                        pass,
                        count,
                        &*(result.to_string()),
                        expected_results[expected_index].3
                    ), // we no longer calculate lamports
                    expected_results[expected_index]
                );
                expected_index += 1;
            }
        }
    }

    #[test]
    #[should_panic(expected = "overflow is detected while summing capitalization")]
    fn test_accountsdb_lamport_overflow() {
        solana_logger::setup();

        let offset = 2;
        let input = vec![
            CalculateHashIntermediate::new(
                0,
                Hash::new_unique(),
                u64::MAX - offset,
                0,
                Pubkey::new_unique(),
            ),
            CalculateHashIntermediate::new(
                0,
                Hash::new_unique(),
                offset + 1,
                0,
                Pubkey::new_unique(),
            ),
        ];
        AccountsHash::de_dup_accounts_in_parallel(&input, 1);
    }

    #[test]
    #[should_panic(expected = "overflow is detected while summing capitalization")]
    fn test_accountsdb_lamport_overflow2() {
        solana_logger::setup();

        let offset = 2;
        let input = vec![
            vec![CalculateHashIntermediate::new(
                0,
                Hash::new_unique(),
                u64::MAX - offset,
                0,
                Pubkey::new_unique(),
            )],
            vec![CalculateHashIntermediate::new(
                0,
                Hash::new_unique(),
                offset + 1,
                0,
                Pubkey::new_unique(),
            )],
        ];
        AccountsHash::de_dup_and_eliminate_zeros(input, &mut HashStats::default());
    }
}
