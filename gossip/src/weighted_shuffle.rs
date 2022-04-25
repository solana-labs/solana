//! The `weighted_shuffle` module provides an iterator over shuffled weights.

use {
    itertools::Itertools,
    num_traits::{CheckedAdd, FromPrimitive, ToPrimitive},
    rand::{
        distributions::uniform::{SampleUniform, UniformSampler},
        Rng, SeedableRng,
    },
    rand_chacha::ChaChaRng,
    std::{
        borrow::Borrow,
        iter,
        ops::{AddAssign, Div, Sub, SubAssign},
    },
};

/// Implements an iterator where indices are shuffled according to their
/// weights:
///   - Returned indices are unique in the range [0, weights.len()).
///   - Higher weighted indices tend to appear earlier proportional to their
///     weight.
///   - Zero weighted indices are shuffled and appear only at the end, after
///     non-zero weighted indices.
#[derive(Clone)]
pub struct WeightedShuffle<T> {
    arr: Vec<T>,       // Underlying array implementing binary indexed tree.
    sum: T,            // Current sum of weights, excluding already selected indices.
    zeros: Vec<usize>, // Indices of zero weighted entries.
}

// The implementation uses binary indexed tree:
// https://en.wikipedia.org/wiki/Fenwick_tree
// to maintain cumulative sum of weights excluding already selected indices
// over self.arr.
impl<T> WeightedShuffle<T>
where
    T: Copy + Default + PartialOrd + AddAssign + CheckedAdd,
{
    /// If weights are negative or overflow the total sum
    /// they are treated as zero.
    pub fn new(name: &'static str, weights: &[T]) -> Self {
        let size = weights.len() + 1;
        let zero = <T as Default>::default();
        let mut arr = vec![zero; size];
        let mut sum = zero;
        let mut zeros = Vec::default();
        let mut num_negative = 0;
        let mut num_overflow = 0;
        for (mut k, &weight) in (1usize..).zip(weights) {
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            // weight < zero does not work for NaNs.
            if !(weight >= zero) {
                zeros.push(k - 1);
                num_negative += 1;
                continue;
            }
            if weight == zero {
                zeros.push(k - 1);
                continue;
            }
            sum = match sum.checked_add(&weight) {
                Some(val) => val,
                None => {
                    zeros.push(k - 1);
                    num_overflow += 1;
                    continue;
                }
            };
            while k < size {
                arr[k] += weight;
                k += k & k.wrapping_neg();
            }
        }
        if num_negative > 0 {
            datapoint_error!("weighted-shuffle-negative", (name, num_negative, i64));
        }
        if num_overflow > 0 {
            datapoint_error!("weighted-shuffle-overflow", (name, num_overflow, i64));
        }
        Self { arr, sum, zeros }
    }
}

impl<T> WeightedShuffle<T>
where
    T: Copy + Default + PartialOrd + AddAssign + SubAssign + Sub<Output = T>,
{
    // Returns cumulative sum of current weights upto index k (inclusive).
    fn cumsum(&self, mut k: usize) -> T {
        let mut out = <T as Default>::default();
        while k != 0 {
            out += self.arr[k];
            k ^= k & k.wrapping_neg();
        }
        out
    }

    // Removes given weight at index k.
    fn remove(&mut self, mut k: usize, weight: T) {
        self.sum -= weight;
        let size = self.arr.len();
        while k < size {
            self.arr[k] -= weight;
            k += k & k.wrapping_neg();
        }
    }

    // Returns smallest index such that self.cumsum(k) > val,
    // along with its respective weight.
    fn search(&self, val: T) -> (/*index:*/ usize, /*weight:*/ T) {
        let zero = <T as Default>::default();
        debug_assert!(val >= zero);
        debug_assert!(val < self.sum);
        let mut lo = (/*index:*/ 0, /*cumsum:*/ zero);
        let mut hi = (self.arr.len() - 1, self.sum);
        while lo.0 + 1 < hi.0 {
            let k = lo.0 + (hi.0 - lo.0) / 2;
            let sum = self.cumsum(k);
            if sum <= val {
                lo = (k, sum);
            } else {
                hi = (k, sum);
            }
        }
        debug_assert!(lo.1 <= val);
        debug_assert!(hi.1 > val);
        (hi.0, hi.1 - lo.1)
    }

    pub fn remove_index(&mut self, index: usize) {
        let zero = <T as Default>::default();
        let weight = self.cumsum(index + 1) - self.cumsum(index);
        if weight != zero {
            self.remove(index + 1, weight);
        } else if let Some(index) = self.zeros.iter().position(|ix| *ix == index) {
            self.zeros.remove(index);
        }
    }
}

impl<T> WeightedShuffle<T>
where
    T: Copy + Default + PartialOrd + AddAssign + SampleUniform + SubAssign + Sub<Output = T>,
{
    // Equivalent to weighted_shuffle.shuffle(&mut rng).next()
    pub fn first<R: Rng>(&self, rng: &mut R) -> Option<usize> {
        let zero = <T as Default>::default();
        if self.sum > zero {
            let sample = <T as SampleUniform>::Sampler::sample_single(zero, self.sum, rng);
            let (index, _weight) = WeightedShuffle::search(self, sample);
            return Some(index - 1);
        }
        if self.zeros.is_empty() {
            return None;
        }
        let index = <usize as SampleUniform>::Sampler::sample_single(0usize, self.zeros.len(), rng);
        self.zeros.get(index).copied()
    }
}

impl<'a, T: 'a> WeightedShuffle<T>
where
    T: Copy + Default + PartialOrd + AddAssign + SampleUniform + SubAssign + Sub<Output = T>,
{
    pub fn shuffle<R: Rng>(mut self, rng: &'a mut R) -> impl Iterator<Item = usize> + 'a {
        std::iter::from_fn(move || {
            let zero = <T as Default>::default();
            if self.sum > zero {
                let sample = <T as SampleUniform>::Sampler::sample_single(zero, self.sum, rng);
                let (index, weight) = WeightedShuffle::search(&self, sample);
                self.remove(index, weight);
                return Some(index - 1);
            }
            if self.zeros.is_empty() {
                return None;
            }
            let index =
                <usize as SampleUniform>::Sampler::sample_single(0usize, self.zeros.len(), rng);
            Some(self.zeros.swap_remove(index))
        })
    }
}

/// Returns a list of indexes shuffled based on the input weights
/// Note - The sum of all weights must not exceed `u64::MAX`
pub fn weighted_shuffle<T, B, F>(weights: F, seed: [u8; 32]) -> Vec<usize>
where
    T: Copy + PartialOrd + iter::Sum + Div<T, Output = T> + FromPrimitive + ToPrimitive,
    B: Borrow<T>,
    F: Iterator<Item = B> + Clone,
{
    let total_weight: T = weights.clone().map(|x| *x.borrow()).sum();
    let mut rng = ChaChaRng::from_seed(seed);
    weights
        .enumerate()
        .map(|(i, weight)| {
            let weight = weight.borrow();
            // This generates an "inverse" weight but it avoids floating point math
            let x = (total_weight / *weight)
                .to_u64()
                .expect("values > u64::max are not supported");
            (
                i,
                // capture the u64 into u128s to prevent overflow
                rng.gen_range(1, u128::from(std::u16::MAX)) * u128::from(x),
            )
        })
        // sort in ascending order
        .sorted_by(|(_, l_val), (_, r_val)| l_val.cmp(r_val))
        .map(|x| x.0)
        .collect()
}

/// Returns the highest index after computing a weighted shuffle.
/// Saves doing any sorting for O(n) max calculation.
// TODO: Remove in favor of rand::distributions::WeightedIndex.
pub fn weighted_best(weights_and_indexes: &[(u64, usize)], seed: [u8; 32]) -> usize {
    if weights_and_indexes.is_empty() {
        return 0;
    }
    let mut rng = ChaChaRng::from_seed(seed);
    let total_weight: u64 = weights_and_indexes.iter().map(|x| x.0).sum();
    let mut lowest_weight = std::u128::MAX;
    let mut best_index = 0;
    for v in weights_and_indexes {
        // This generates an "inverse" weight but it avoids floating point math
        let x = (total_weight / v.0)
            .to_u64()
            .expect("values > u64::max are not supported");
        // capture the u64 into u128s to prevent overflow
        let computed_weight = rng.gen_range(1, u128::from(std::u16::MAX)) * u128::from(x);
        // The highest input weight maps to the lowest computed weight
        if computed_weight < lowest_weight {
            lowest_weight = computed_weight;
            best_index = v.1;
        }
    }

    best_index
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{convert::TryInto, iter::repeat_with},
    };

    fn weighted_shuffle_slow<R>(rng: &mut R, mut weights: Vec<u64>) -> Vec<usize>
    where
        R: Rng,
    {
        let mut shuffle = Vec::with_capacity(weights.len());
        let mut high: u64 = weights.iter().sum();
        let mut zeros: Vec<_> = weights
            .iter()
            .enumerate()
            .filter(|(_, w)| **w == 0)
            .map(|(i, _)| i)
            .collect();
        while high != 0 {
            let sample = rng.gen_range(0, high);
            let index = weights
                .iter()
                .scan(0, |acc, &w| {
                    *acc += w;
                    Some(*acc)
                })
                .position(|acc| sample < acc)
                .unwrap();
            shuffle.push(index);
            high -= weights[index];
            weights[index] = 0;
        }
        while !zeros.is_empty() {
            let index = <usize as SampleUniform>::Sampler::sample_single(0usize, zeros.len(), rng);
            shuffle.push(zeros.swap_remove(index));
        }
        shuffle
    }

    #[test]
    fn test_weighted_shuffle_iterator() {
        let mut test_set = [0; 6];
        let mut count = 0;
        let shuffle = weighted_shuffle(vec![50, 10, 2, 1, 1, 1].into_iter(), [0x5a; 32]);
        shuffle.into_iter().for_each(|x| {
            assert_eq!(test_set[x], 0);
            test_set[x] = 1;
            count += 1;
        });
        assert_eq!(count, 6);
    }

    #[test]
    fn test_weighted_shuffle_iterator_large() {
        let mut test_set = [0; 100];
        let mut test_weights = vec![0; 100];
        (0..100).for_each(|i| test_weights[i] = (i + 1) as u64);
        let mut count = 0;
        let shuffle = weighted_shuffle(test_weights.into_iter(), [0xa5; 32]);
        shuffle.into_iter().for_each(|x| {
            assert_eq!(test_set[x], 0);
            test_set[x] = 1;
            count += 1;
        });
        assert_eq!(count, 100);
    }

    #[test]
    fn test_weighted_shuffle_compare() {
        let shuffle = weighted_shuffle(vec![50, 10, 2, 1, 1, 1].into_iter(), [0x5a; 32]);

        let shuffle1 = weighted_shuffle(vec![50, 10, 2, 1, 1, 1].into_iter(), [0x5a; 32]);
        shuffle1
            .into_iter()
            .zip(shuffle.into_iter())
            .for_each(|(x, y)| {
                assert_eq!(x, y);
            });
    }

    #[test]
    fn test_weighted_shuffle_imbalanced() {
        let mut weights = vec![std::u32::MAX as u64; 3];
        weights.push(1);
        let shuffle = weighted_shuffle(weights.iter().cloned(), [0x5a; 32]);
        shuffle.into_iter().for_each(|x| {
            if x == weights.len() - 1 {
                assert_eq!(weights[x], 1);
            } else {
                assert_eq!(weights[x], std::u32::MAX as u64);
            }
        });
    }

    #[test]
    fn test_weighted_best() {
        let weights_and_indexes: Vec<_> = vec![100u64, 1000, 10_000, 10]
            .into_iter()
            .enumerate()
            .map(|(i, weight)| (weight, i))
            .collect();
        let best_index = weighted_best(&weights_and_indexes, [0x5b; 32]);
        assert_eq!(best_index, 2);
    }

    // Asserts that empty weights will return empty shuffle.
    #[test]
    fn test_weighted_shuffle_empty_weights() {
        let weights = Vec::<u64>::new();
        let mut rng = rand::thread_rng();
        let shuffle = WeightedShuffle::new("", &weights);
        assert!(shuffle.clone().shuffle(&mut rng).next().is_none());
        assert!(shuffle.first(&mut rng).is_none());
    }

    // Asserts that zero weights will be shuffled.
    #[test]
    fn test_weighted_shuffle_zero_weights() {
        let weights = vec![0u64; 5];
        let seed = [37u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);
        let shuffle = WeightedShuffle::new("", &weights);
        assert_eq!(
            shuffle.clone().shuffle(&mut rng).collect::<Vec<_>>(),
            [1, 4, 2, 3, 0]
        );
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(shuffle.first(&mut rng), Some(1));
    }

    // Asserts that each index is selected proportional to its weight.
    #[test]
    fn test_weighted_shuffle_sanity() {
        let seed: Vec<_> = (1..).step_by(3).take(32).collect();
        let seed: [u8; 32] = seed.try_into().unwrap();
        let mut rng = ChaChaRng::from_seed(seed);
        let weights = [1, 0, 1000, 0, 0, 10, 100, 0];
        let mut counts = [0; 8];
        for _ in 0..100000 {
            let mut shuffle = WeightedShuffle::new("", &weights).shuffle(&mut rng);
            counts[shuffle.next().unwrap()] += 1;
            let _ = shuffle.count(); // consume the rest.
        }
        assert_eq!(counts, [95, 0, 90069, 0, 0, 908, 8928, 0]);
        let mut counts = [0; 8];
        for _ in 0..100000 {
            let mut shuffle = WeightedShuffle::new("", &weights);
            shuffle.remove_index(5);
            shuffle.remove_index(3);
            shuffle.remove_index(1);
            let mut shuffle = shuffle.shuffle(&mut rng);
            counts[shuffle.next().unwrap()] += 1;
            let _ = shuffle.count(); // consume the rest.
        }
        assert_eq!(counts, [97, 0, 90862, 0, 0, 0, 9041, 0]);
    }

    #[test]
    fn test_weighted_shuffle_negative_overflow() {
        const SEED: [u8; 32] = [48u8; 32];
        let weights = [19i64, 23, 7, 0, 0, 23, 3, 0, 5, 0, 19, 29];
        let mut rng = ChaChaRng::from_seed(SEED);
        let shuffle = WeightedShuffle::new("", &weights);
        assert_eq!(
            shuffle.shuffle(&mut rng).collect::<Vec<_>>(),
            [8, 1, 5, 10, 11, 0, 2, 6, 9, 4, 3, 7]
        );
        // Negative weights and overflowing ones are treated as zero.
        let weights = [19, 23, 7, -57, i64::MAX, 23, 3, i64::MAX, 5, -79, 19, 29];
        let mut rng = ChaChaRng::from_seed(SEED);
        let shuffle = WeightedShuffle::new("", &weights);
        assert_eq!(
            shuffle.shuffle(&mut rng).collect::<Vec<_>>(),
            [8, 1, 5, 10, 11, 0, 2, 6, 9, 4, 3, 7]
        );
    }

    #[test]
    fn test_weighted_shuffle_hard_coded() {
        let weights = [
            78, 70, 38, 27, 21, 0, 82, 42, 21, 77, 77, 0, 17, 4, 50, 96, 0, 83, 33, 16, 72,
        ];
        let seed = [48u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);
        let mut shuffle = WeightedShuffle::new("", &weights);
        assert_eq!(
            shuffle.clone().shuffle(&mut rng).collect::<Vec<_>>(),
            [2, 12, 18, 0, 14, 15, 17, 10, 1, 9, 7, 6, 13, 20, 4, 19, 3, 8, 11, 16, 5]
        );
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(shuffle.first(&mut rng), Some(2));
        let mut rng = ChaChaRng::from_seed(seed);
        shuffle.remove_index(11);
        shuffle.remove_index(3);
        shuffle.remove_index(15);
        shuffle.remove_index(0);
        assert_eq!(
            shuffle.clone().shuffle(&mut rng).collect::<Vec<_>>(),
            [4, 6, 1, 12, 19, 14, 17, 20, 2, 9, 10, 8, 7, 18, 13, 5, 16]
        );
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(shuffle.first(&mut rng), Some(4));
        let seed = [37u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);
        let mut shuffle = WeightedShuffle::new("", &weights);
        assert_eq!(
            shuffle.clone().shuffle(&mut rng).collect::<Vec<_>>(),
            [19, 3, 15, 14, 6, 10, 17, 18, 9, 2, 4, 1, 0, 7, 8, 20, 12, 13, 16, 5, 11]
        );
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(shuffle.first(&mut rng), Some(19));
        shuffle.remove_index(16);
        shuffle.remove_index(8);
        shuffle.remove_index(20);
        shuffle.remove_index(5);
        shuffle.remove_index(19);
        shuffle.remove_index(4);
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(
            shuffle.clone().shuffle(&mut rng).collect::<Vec<_>>(),
            [17, 2, 9, 14, 6, 10, 12, 1, 15, 13, 7, 0, 18, 3, 11]
        );
        let mut rng = ChaChaRng::from_seed(seed);
        assert_eq!(shuffle.first(&mut rng), Some(17));
    }

    #[test]
    fn test_weighted_shuffle_match_slow() {
        let mut rng = rand::thread_rng();
        let weights: Vec<u64> = repeat_with(|| rng.gen_range(0, 1000)).take(997).collect();
        for _ in 0..10 {
            let mut seed = [0u8; 32];
            rng.fill(&mut seed[..]);
            let mut rng = ChaChaRng::from_seed(seed);
            let shuffle = WeightedShuffle::new("", &weights);
            let shuffle: Vec<_> = shuffle.shuffle(&mut rng).collect();
            let mut rng = ChaChaRng::from_seed(seed);
            let shuffle_slow = weighted_shuffle_slow(&mut rng, weights.clone());
            assert_eq!(shuffle, shuffle_slow);
            let mut rng = ChaChaRng::from_seed(seed);
            let shuffle = WeightedShuffle::new("", &weights);
            assert_eq!(shuffle.first(&mut rng), Some(shuffle_slow[0]));
        }
    }
}
