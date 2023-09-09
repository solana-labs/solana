/// There are 10^6 micro-lamports in one lamport
const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

type MicroLamports = u128;

pub enum PrioritizationFeeType {
    ComputeUnitPrice(u64),
    // TODO: remove 'Deprecated' after feature remove_deprecated_request_unit_ix::id() is activated
    Deprecated(u64),
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct PrioritizationFeeDetails {
    fee: u64,
    priority: u64,
}

impl PrioritizationFeeDetails {
    pub fn new(fee_type: PrioritizationFeeType, compute_unit_limit: u64) -> Self {
        match fee_type {
            // TODO: remove support of 'Deprecated' after feature remove_deprecated_request_unit_ix::id() is activated
            PrioritizationFeeType::Deprecated(fee) => {
                let micro_lamport_fee: MicroLamports =
                    (fee as u128).saturating_mul(MICRO_LAMPORTS_PER_LAMPORT as u128);
                let priority = micro_lamport_fee
                    .checked_div(compute_unit_limit as u128)
                    .map(|priority| u64::try_from(priority).unwrap_or(u64::MAX))
                    .unwrap_or(0);

                Self { fee, priority }
            }
            PrioritizationFeeType::ComputeUnitPrice(cu_price) => {
                let micro_lamport_fee: MicroLamports =
                    (cu_price as u128).saturating_mul(compute_unit_limit as u128);
                let fee = micro_lamport_fee
                    .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
                    .checked_div(MICRO_LAMPORTS_PER_LAMPORT as u128)
                    .and_then(|fee| u64::try_from(fee).ok())
                    .unwrap_or(u64::MAX);

                Self {
                    fee,
                    priority: cu_price,
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
        for compute_units in [0, 1, MICRO_LAMPORTS_PER_LAMPORT, u64::MAX] {
            assert_eq!(
                FeeDetails::new(FeeType::ComputeUnitPrice(0), compute_units),
                FeeDetails::default(),
            );
            assert_eq!(
                FeeDetails::new(FeeType::Deprecated(0), compute_units),
                FeeDetails::default(),
            );
        }
    }

    #[test]
    fn test_new_with_compute_unit_price() {
        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT - 1), 1),
            FeeDetails {
                fee: 1,
                priority: MICRO_LAMPORTS_PER_LAMPORT - 1,
            },
            "should round up (<1.0) lamport fee to 1 lamport"
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT), 1),
            FeeDetails {
                fee: 1,
                priority: MICRO_LAMPORTS_PER_LAMPORT,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT + 1), 1),
            FeeDetails {
                fee: 2,
                priority: MICRO_LAMPORTS_PER_LAMPORT + 1,
            },
            "should round up (>1.0) lamport fee to 2 lamports"
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(200), 100_000),
            FeeDetails {
                fee: 20,
                priority: 200,
            },
        );

        assert_eq!(
            FeeDetails::new(
                FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT),
                u64::MAX
            ),
            FeeDetails {
                fee: u64::MAX,
                priority: MICRO_LAMPORTS_PER_LAMPORT,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(u64::MAX), u64::MAX),
            FeeDetails {
                fee: u64::MAX,
                priority: u64::MAX,
            },
        );
    }

    #[test]
    fn test_new_with_deprecated_fee() {
        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), MICRO_LAMPORTS_PER_LAMPORT / 2 - 1),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
            "should round down fee rate of (>2.0) to priority value 1"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), MICRO_LAMPORTS_PER_LAMPORT / 2),
            FeeDetails {
                fee: 1,
                priority: 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), MICRO_LAMPORTS_PER_LAMPORT / 2 + 1),
            FeeDetails {
                fee: 1,
                priority: 1,
            },
            "should round down fee rate of (<2.0) to priority value 1"
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(1), MICRO_LAMPORTS_PER_LAMPORT),
            FeeDetails {
                fee: 1,
                priority: 1,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(42), 42 * MICRO_LAMPORTS_PER_LAMPORT),
            FeeDetails {
                fee: 42,
                priority: 1,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(420), 42 * MICRO_LAMPORTS_PER_LAMPORT),
            FeeDetails {
                fee: 420,
                priority: 10,
            },
        );

        assert_eq!(
            FeeDetails::new(
                FeeType::Deprecated(u64::MAX),
                2 * MICRO_LAMPORTS_PER_LAMPORT
            ),
            FeeDetails {
                fee: u64::MAX,
                priority: u64::MAX / 2,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::Deprecated(u64::MAX), u64::MAX),
            FeeDetails {
                fee: u64::MAX,
                priority: MICRO_LAMPORTS_PER_LAMPORT,
            },
        );
    }
}
