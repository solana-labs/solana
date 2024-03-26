/// There are 10^6 micro-lamports in one lamport
const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

type MicroLamports = u128;

pub enum PrioritizationFeeType {
    ComputeUnitPrice(u64),
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct PrioritizationFeeDetails {
    fee: u64,
    compute_unit_price: u64,
}

impl PrioritizationFeeDetails {
    pub fn new(fee_type: PrioritizationFeeType, compute_unit_limit: u64) -> Self {
        match fee_type {
            PrioritizationFeeType::ComputeUnitPrice(compute_unit_price) => {
                let micro_lamport_fee: MicroLamports =
                    (compute_unit_price as u128).saturating_mul(compute_unit_limit as u128);
                let fee = micro_lamport_fee
                    .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
                    .checked_div(MICRO_LAMPORTS_PER_LAMPORT as u128)
                    .and_then(|fee| u64::try_from(fee).ok())
                    .unwrap_or(u64::MAX);

                Self {
                    fee,
                    compute_unit_price,
                }
            }
        }
    }

    pub fn get_fee(&self) -> u64 {
        self.fee
    }

    pub fn get_compute_unit_price(&self) -> u64 {
        self.compute_unit_price
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
        }
    }

    #[test]
    fn test_new_with_compute_unit_price() {
        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT - 1), 1),
            FeeDetails {
                fee: 1,
                compute_unit_price: MICRO_LAMPORTS_PER_LAMPORT - 1,
            },
            "should round up (<1.0) lamport fee to 1 lamport"
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT), 1),
            FeeDetails {
                fee: 1,
                compute_unit_price: MICRO_LAMPORTS_PER_LAMPORT,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT + 1), 1),
            FeeDetails {
                fee: 2,
                compute_unit_price: MICRO_LAMPORTS_PER_LAMPORT + 1,
            },
            "should round up (>1.0) lamport fee to 2 lamports"
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(200), 100_000),
            FeeDetails {
                fee: 20,
                compute_unit_price: 200,
            },
        );

        assert_eq!(
            FeeDetails::new(
                FeeType::ComputeUnitPrice(MICRO_LAMPORTS_PER_LAMPORT),
                u64::MAX
            ),
            FeeDetails {
                fee: u64::MAX,
                compute_unit_price: MICRO_LAMPORTS_PER_LAMPORT,
            },
        );

        assert_eq!(
            FeeDetails::new(FeeType::ComputeUnitPrice(u64::MAX), u64::MAX),
            FeeDetails {
                fee: u64::MAX,
                compute_unit_price: u64::MAX,
            },
        );
    }
}
