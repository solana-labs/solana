//! Iterators for RollingBitField

use {super::RollingBitField, std::ops::Range};

/// Iterate over the 'set' bits of a RollingBitField
#[derive(Debug)]
pub struct RollingBitFieldOnesIter<'a> {
    rolling_bit_field: &'a RollingBitField,
    excess_iter: std::collections::hash_set::Iter<'a, u64>,
    bit_range: Range<u64>,
}

impl<'a> RollingBitFieldOnesIter<'a> {
    #[must_use]
    pub fn new(rolling_bit_field: &'a RollingBitField) -> Self {
        Self {
            rolling_bit_field,
            excess_iter: rolling_bit_field.excess.iter(),
            bit_range: rolling_bit_field.min..rolling_bit_field.max_exclusive,
        }
    }
}

impl Iterator for RollingBitFieldOnesIter<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate over the excess first
        if let Some(excess) = self.excess_iter.next() {
            return Some(*excess);
        }

        // Then iterate over the bit vec
        loop {
            // If there are no more bits in the range, then we've iterated over everything and are done
            let bit = self.bit_range.next()?;

            if self.rolling_bit_field.contains_assume_in_range(&bit) {
                break Some(bit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, test_case::test_case};

    #[test_case(128, vec![]; "empty")]
    #[test_case(128, vec![128_007, 128_017, 128_107]; "without excess")]
    #[test_case(128, vec![128_007, 128_017, 128_107, 3, 30, 300]; "with excess")]
    // Even though these values are within the range, in an absolute sense,
    // they will wrap around after multiples of 16.
    #[test_case(16, vec![35, 40, 45                    ])]
    #[test_case(16, vec![    40, 45, 50                ])]
    #[test_case(16, vec![        45, 50, 55            ])]
    #[test_case(16, vec![            50, 55, 60        ])]
    #[test_case(16, vec![                55, 60, 65    ])]
    #[test_case(16, vec![                    60, 65, 70])]
    fn test_rolling_bit_field_ones_iter(num_bits: u64, mut expected: Vec<u64>) {
        let mut rolling_bit_field = RollingBitField::new(num_bits);
        for val in &expected {
            rolling_bit_field.insert(*val);
        }

        let mut actual: Vec<_> = rolling_bit_field.iter_ones().collect();

        // Since iteration order of the 'excess' is not deterministic, sort the 'actual'
        // and 'expected' vectors to ensure they can compare deterministically.
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }
}
