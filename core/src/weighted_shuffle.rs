//! The `weighted_shuffle` module provides an iterator over shuffled weights.

use rand::distributions::uniform::SampleUniform;
use rand::distributions::{Distribution, WeightedIndex};
use rand_chacha::ChaChaRng;

pub struct WeightedShuffle<
    T: SampleUniform + PartialOrd + for<'a> ::core::ops::AddAssign<&'a T> + Clone + Default,
> {
    weights: Vec<T>,
    rng: ChaChaRng,
}

impl<T: SampleUniform + PartialOrd + for<'a> ::core::ops::AddAssign<&'a T> + Clone + Default>
    WeightedShuffle<T>
{
    pub fn new(weights: Vec<T>, rng: ChaChaRng) -> Self {
        WeightedShuffle { weights, rng }
    }
}

impl<T: SampleUniform + PartialOrd + for<'a> ::core::ops::AddAssign<&'a T> + Clone + Default>
    Iterator for WeightedShuffle<T>
{
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        match WeightedIndex::new(&self.weights) {
            Ok(dist) => {
                let choice = dist.sample(&mut self.rng);
                self.weights[choice] = <T as Default>::default();
                Some(choice)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn test_weighted_shuffle_iterator() {
        let mut test_set = [0; 6];
        let mut count = 0;
        let shuffle: WeightedShuffle<u64> =
            WeightedShuffle::new(vec![50, 10, 2, 1, 1, 1], ChaChaRng::from_seed([0x5a; 32]));
        shuffle.into_iter().for_each(|x| {
            assert_eq!(test_set[x], 0);
            test_set[x] = 1;
            count += 1;
        });
        assert_eq!(count, 6);
    }
}
