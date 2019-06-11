use crate::message::Message;
use log::*;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeeCalculator {
    // The current cost of a signature  This amount may increase/decrease over time based on
    // cluster processing load.
    pub lamports_per_signature: u64,

    // The target cost of a signature when the cluster is operating around target_signatures_per_slot
    // signatures
    pub target_lamports_per_signature: u64,

    // Used to estimate the desired processing capacity of the cluster.  As the signatures for
    // recent slots are fewer/greater than this value, lamports_per_signature will decrease/increase
    // for the next slot.  A value of 0 disables lamports_per_signature fee adjustments
    pub target_signatures_per_slot: usize,

    #[serde(skip)]
    pub min_lamports_per_signature: u64,
    #[serde(skip)]
    pub max_lamports_per_signature: u64,
}

impl FeeCalculator {
    pub fn new(target_lamports_per_signature: u64) -> Self {
        let base_fee_calculator = Self {
            target_lamports_per_signature,
            lamports_per_signature: target_lamports_per_signature,
            ..FeeCalculator::default()
        };

        Self::new_derived(&base_fee_calculator, 0)
    }

    pub fn new_derived(
        base_fee_calculator: &FeeCalculator,
        latest_signatures_per_slot: usize,
    ) -> Self {
        let mut me = base_fee_calculator.clone();

        if me.target_signatures_per_slot > 0 {
            // lamports_per_signature can range from 50% to 1000% of
            // target_lamports_per_signature
            //
            // TODO: Are these decent limits?
            //
            me.min_lamports_per_signature = std::cmp::max(1, me.target_lamports_per_signature / 2);
            me.max_lamports_per_signature = me.target_lamports_per_signature * 10;

            // What the cluster should charge at `latest_signatures_per_slot`
            let desired_lamports_per_signature = std::cmp::min(
                me.max_lamports_per_signature,
                std::cmp::max(
                    me.min_lamports_per_signature,
                    me.target_lamports_per_signature
                        * std::cmp::min(latest_signatures_per_slot, std::u32::MAX as usize) as u64
                        / me.target_signatures_per_slot as u64,
                ),
            );

            trace!(
                "desired_lamports_per_signature: {}",
                desired_lamports_per_signature
            );

            let gap = desired_lamports_per_signature as i64
                - base_fee_calculator.lamports_per_signature as i64;

            if gap == 0 {
                me.lamports_per_signature = desired_lamports_per_signature;
            } else {
                // Adjust fee by 5% of target_lamports_per_signature to produce a smooth increase/decrease in fees over time.
                //
                // TODO: Is this fee curve smooth enough or too smooth?
                //
                let gap_adjust =
                    std::cmp::max(1, me.target_lamports_per_signature as i64 / 20) * gap.signum();

                trace!(
                    "lamports_per_signature gap is {}, adjusting by {}",
                    gap,
                    gap_adjust
                );

                me.lamports_per_signature = std::cmp::min(
                    me.max_lamports_per_signature,
                    std::cmp::max(
                        me.min_lamports_per_signature,
                        (base_fee_calculator.lamports_per_signature as i64 + gap_adjust) as u64,
                    ),
                );
            }
        } else {
            me.lamports_per_signature = base_fee_calculator.target_lamports_per_signature;
        }
        debug!(
            "new_derived(): lamports_per_signature: {}",
            me.lamports_per_signature
        );
        me
    }

    pub fn calculate_fee(&self, message: &Message) -> u64 {
        self.lamports_per_signature * u64::from(message.header.num_required_signatures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pubkey::Pubkey;
    use crate::system_instruction;

    #[test]
    fn test_fee_calculator_calculate_fee() {
        // Default: no fee.
        let message = Message::new(vec![]);
        assert_eq!(FeeCalculator::default().calculate_fee(&message), 0);

        // No signature, no fee.
        assert_eq!(FeeCalculator::new(1).calculate_fee(&message), 0);

        // One signature, a fee.
        let pubkey0 = Pubkey::new(&[0; 32]);
        let pubkey1 = Pubkey::new(&[1; 32]);
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let message = Message::new(vec![ix0]);
        assert_eq!(FeeCalculator::new(2).calculate_fee(&message), 2);

        // Two signatures, double the fee.
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let ix1 = system_instruction::transfer(&pubkey1, &pubkey0, 1);
        let message = Message::new(vec![ix0, ix1]);
        assert_eq!(FeeCalculator::new(2).calculate_fee(&message), 4);
    }

    #[test]
    fn test_fee_calculator_derived_default() {
        solana_logger::setup();

        let mut f0 = FeeCalculator::default();
        assert_eq!(f0.target_signatures_per_slot, 0);
        assert_eq!(f0.target_lamports_per_signature, 0);
        assert_eq!(f0.lamports_per_signature, 0);
        f0.target_lamports_per_signature = 42;

        let f1 = FeeCalculator::new_derived(&f0, 4242);
        assert_eq!(f1.target_signatures_per_slot, 0);
        assert_eq!(f1.target_lamports_per_signature, 42);
        assert_eq!(f1.lamports_per_signature, 42);
    }

    #[test]
    fn test_fee_calculator_derived_adjust() {
        solana_logger::setup();

        let mut f = FeeCalculator::default();
        f.target_lamports_per_signature = 100;
        f.target_signatures_per_slot = 100;
        f = FeeCalculator::new_derived(&f, 0);

        // Ramp fees up
        while f.lamports_per_signature < f.max_lamports_per_signature {
            f = FeeCalculator::new_derived(&f, std::usize::MAX);
            info!("[up] f.lamports_per_signature={}", f.lamports_per_signature);
        }

        // Ramp fees down
        while f.lamports_per_signature > f.min_lamports_per_signature {
            f = FeeCalculator::new_derived(&f, 0);
            info!(
                "[down] f.lamports_per_signature={}",
                f.lamports_per_signature
            );
        }

        // Arrive at target rate
        while f.lamports_per_signature != f.target_lamports_per_signature {
            f = FeeCalculator::new_derived(&f, f.target_signatures_per_slot);
            info!(
                "[target] f.lamports_per_signature={}",
                f.lamports_per_signature
            );
        }
    }
}
