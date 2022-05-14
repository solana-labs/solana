// Prioritization fee rate is 1 lamports per 10K CUs
const COMPUTE_UNIT_TICK_SIZE: u64 = 10_000;

pub enum PrioritizationFeeType {
    Rate(u64),
    Deprecated(u64),
}

#[derive(Default, Debug, PartialEq)]
pub struct PrioritizationFeeDetails {
    fee: u64,
    priority: u64,
}

impl PrioritizationFeeDetails {
    pub fn new(fee_type: PrioritizationFeeType, max_compute_units: u64) -> Self {
        let mut compute_ticks = max_compute_units
            .saturating_div(COMPUTE_UNIT_TICK_SIZE)
            .max(1);
        match fee_type {
            PrioritizationFeeType::Deprecated(fee) => Self {
                fee,
                priority: fee.saturating_div(compute_ticks),
            },
            PrioritizationFeeType::Rate(fee_rate) => {
                if compute_ticks.saturating_mul(COMPUTE_UNIT_TICK_SIZE) < max_compute_units {
                    compute_ticks = compute_ticks.saturating_add(1);
                }

                Self {
                    fee: fee_rate.saturating_mul(compute_ticks),
                    priority: fee_rate,
                }
            }
        }
    }

    pub fn get_fee(&self) -> u64 {
        self.fee
    }

    pub fn get_priority(&self) -> u64 {
        self.priority
    }
}

#[cfg(test)]
mod test {
    use super::{PrioritizationFeeDetails as FeeDetails, PrioritizationFeeType as FeeType, *};

    #[test]
    fn test_new_with_no_fee() {
        for compute_units in [0, 1, COMPUTE_UNIT_TICK_SIZE, u64::MAX] {
            assert_eq!(
                FeeDetails::new(FeeType::Rate(0), compute_units),
                FeeDetails::default(),
            );
            assert_eq!(
                FeeDetails::new(FeeType::Deprecated(0), compute_units),
                FeeDetails::default(),
            );
        }
    }

    #[test]
    fn test_new_with_one_compute_tick() {
        for compute_units in [
            0, // zero compute units should be rounded up to 1 tick
            1, // compute units are rounded up to the nearest tick
            COMPUTE_UNIT_TICK_SIZE,
        ] {
            for fee_type_value in [0, 1, 100, u64::MAX] {
                let expected_details = FeeDetails {
                    fee: fee_type_value,
                    priority: fee_type_value,
                };
                assert_eq!(
                    FeeDetails::new(FeeType::Rate(fee_type_value), compute_units),
                    expected_details,
                );

                assert_eq!(
                    FeeDetails::new(FeeType::Deprecated(fee_type_value), compute_units),
                    expected_details,
                );
            }
        }
    }

    #[test]
    fn test_new_with_fee_rate() {
        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 2 * 42,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), 42 * COMPUTE_UNIT_TICK_SIZE - 1),
            FeeDetails {
                fee: 2 * 42,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(u64::MAX), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: u64::MAX,
                priority: u64::MAX,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(u64::MAX), u64::MAX),
            FeeDetails {
                fee: u64::MAX,
                priority: u64::MAX,
            },
        );
    }

    #[test]
    fn test_new_with_deprecated_fee() {
        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(2), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 2,
                priority: 0,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(42), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 42,
                priority: 1,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(42), 42 * COMPUTE_UNIT_TICK_SIZE - 1),
            FeeDetails {
                fee: 42,
                priority: 1,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(42), 42 * COMPUTE_UNIT_TICK_SIZE + 1),
            FeeDetails {
                fee: 42,
                priority: 1,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(420), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 420,
                priority: 10,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(u64::MAX), 2 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: u64::MAX,
                priority: u64::MAX / 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(u64::MAX), u64::MAX),
            FeeDetails {
                fee: u64::MAX,
                priority: COMPUTE_UNIT_TICK_SIZE,
            },
        );
    }
}
