//! The `weighted_shuffle` module provides an iterator over shuffled weights.

use itertools::Itertools;
use num_traits::{FromPrimitive, ToPrimitive};
use rand::Rng;
use rand_chacha::ChaChaRng;
use std::iter;
use std::ops::Div;

/// Returns a list of indexes shuffled based on the input weights
/// Note - The sum of all weights must not exceed `u64::MAX`
pub fn weighted_shuffle<T>(weights: Vec<T>, mut rng: ChaChaRng) -> Vec<usize>
where
    T: Copy + PartialOrd + iter::Sum + Div<T, Output = T> + FromPrimitive + ToPrimitive,
{
    let total_weight: T = weights.clone().into_iter().sum();
    weights
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            // This generates an "inverse" weight but it avoids floating point math
            let x = (total_weight / v)
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
pub fn weighted_best(weights_and_indexes: &[(u64, usize)], mut rng: ChaChaRng) -> usize {
    if weights_and_indexes.is_empty() {
        return 0;
    }
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
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn test_weighted_shuffle_iterator() {
        let mut test_set = [0; 6];
        let mut count = 0;
        let shuffle = weighted_shuffle(vec![50, 10, 2, 1, 1, 1], ChaChaRng::from_seed([0x5a; 32]));
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
        let shuffle = weighted_shuffle(test_weights, ChaChaRng::from_seed([0xa5; 32]));
        shuffle.into_iter().for_each(|x| {
            assert_eq!(test_set[x], 0);
            test_set[x] = 1;
            count += 1;
        });
        assert_eq!(count, 100);
    }

    #[test]
    fn test_weighted_shuffle_compare() {
        let shuffle = weighted_shuffle(vec![50, 10, 2, 1, 1, 1], ChaChaRng::from_seed([0x5a; 32]));

        let shuffle1 = weighted_shuffle(vec![50, 10, 2, 1, 1, 1], ChaChaRng::from_seed([0x5a; 32]));
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
        let shuffle = weighted_shuffle(weights.clone(), ChaChaRng::from_seed([0x5a; 32]));
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
        let best_index = weighted_best(&weights_and_indexes, ChaChaRng::from_seed([0x5b; 32]));
        assert_eq!(best_index, 2);
    }
}
