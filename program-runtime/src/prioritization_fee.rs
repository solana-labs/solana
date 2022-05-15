// Prioritization fee rate is measured in lamports per compute unit "ticks"
const COMPUTE_UNIT_TICK_SIZE: u64 = 10_000;

// `COMPUTE_UNIT_TICK_SIZE` tick lamports = 1 lamport
type TickLamports = u128;

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
        match fee_type {
            PrioritizationFeeType::Deprecated(fee) => {
                let priority = if max_compute_units == 0 {
                    0
                } else {
                    let tick_lamport_fee: TickLamports =
                        (fee as u128).saturating_mul(COMPUTE_UNIT_TICK_SIZE as u128);
                    let priority = tick_lamport_fee.saturating_div(max_compute_units as u128);
                    u64::try_from(priority).unwrap_or(u64::MAX)
                };

                Self { fee, priority }
            }
            PrioritizationFeeType::Rate(fee_rate) => {
                let fee = {
                    let tick_lamport_fee: TickLamports =
                        (fee_rate as u128).saturating_mul(max_compute_units as u128);
                    let mut fee = tick_lamport_fee.saturating_div(COMPUTE_UNIT_TICK_SIZE as u128);
                    if fee.saturating_mul(COMPUTE_UNIT_TICK_SIZE as u128) < tick_lamport_fee {
                        fee = fee.saturating_add(1);
                    }
                    u64::try_from(fee).unwrap_or(u64::MAX)
                };

                Self {
                    fee,
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
    fn test_new_with_fee_rate() {
        assert!(COMPUTE_UNIT_TICK_SIZE % 2 == 0);
        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), COMPUTE_UNIT_TICK_SIZE / 2 - 1),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
            "should round up 2 * (<0.5) lamport fee to 1 lamport"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), COMPUTE_UNIT_TICK_SIZE / 2),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), COMPUTE_UNIT_TICK_SIZE / 2 + 1),
            FeeDetails {
                fee: 2,
                priority: 2,
            },
            "should round up 2 * (>0.5) lamport fee to 2 lamports"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 2,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(2), 42 * COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 42 * 2,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Rate(u64::MAX), COMPUTE_UNIT_TICK_SIZE),
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
            FeeDetails::new(FeeType::Deprecated(1), COMPUTE_UNIT_TICK_SIZE / 2 - 1),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
            "should round down fee rate of (1 / (<0.5 compute ticks)) to priority value 2"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), COMPUTE_UNIT_TICK_SIZE / 2),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), COMPUTE_UNIT_TICK_SIZE / 2 + 1),
            FeeDetails {
                fee: 1,
                priority: 1,
            },
            "should round down fee rate of (1 / (>0.5 compute ticks)) to priority value 1"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), COMPUTE_UNIT_TICK_SIZE),
            FeeDetails {
                fee: 1,
                priority: 1,
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
