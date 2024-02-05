use{
    crate::ema::AggregatedVarianceStats,
    solana_sdk::clock::Slot,
};

#[derive(Clone, Debug)]
pub struct ComputeUnitPricer {
    /// only for exprimenting println!
    pub slot: Slot,

    /// moving average cu_utilization read from previous blocks in percentage (10 means 10%);
    /// this block's tracking stats contribute to next block's average cu_utilization
    pub cu_utilization: AggregatedVarianceStats,

    /// milli-lamports per CU. The rate dynamically floats based on cu_utilization. In general,
    ///    if cu_utilization > target_utilization, increase the cu_price by 1.01x
    ///    if cu_utilization < target_utilization, decrease the cu_price by 0.99x
    /// it starts w 1 milli-lamport/cu, stored as 1_000 microlamports internally for integer
    /// arithmatic.
    pub cu_price: u64,
}

const NORMAL_CU_PRICE: u64 = 1_000; // micro_lamports/CU
pub const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;

const PRICE_CHANGE_RATE: u64 = 10;    // = 10/1_000 = 1%
const PRICE_CHANGE_SCALE: u64 = 1_000;

// the target utilization ema, some offline script on mnb logs gave me 81% as `mean` utilization
// SIMD-0110 calls 50% at initial target
const TARGET_UTILIZATION: u64 = 50;
// the distance from target utilization that should be considered as "normal",
// set to be 2*stddev of raw block utilization (from same offline script did before)
// SIMD-0110 calls 0 to start with
const UTILIZATION_BAND_WIDTH: u64 = 0;
const CU_UTILIZATION_UPPER_BOUND: u64 = TARGET_UTILIZATION + UTILIZATION_BAND_WIDTH;
const CU_UTILIZATION_LOWER_BOUND: u64 = TARGET_UTILIZATION - UTILIZATION_BAND_WIDTH;

// NOTE, not setting MIN/MAX cu_price yet for expriment, perhaps a good idea to have them when go
// out of exprimenting
//

impl Default for ComputeUnitPricer {
    fn default() -> Self {
        Self {
            slot: 0,
            cu_utilization: AggregatedVarianceStats::new_with_initial_ema(TARGET_UTILIZATION),
            cu_price: NORMAL_CU_PRICE,
        }
    }
}

impl ComputeUnitPricer {
    // return fee_rate in micro-lamports/cu
    pub fn get_fee_rate_micro_lamports_per_cu(&self) -> u64 {
        self.cu_price
    }

    // use currently cu_price to calculate total fee in lamports
    #[allow(dead_code)]
    pub fn calculate_fee(&self, compute_units: u64) -> u64 {
        (compute_units as u128)
        .saturating_mul(self.cu_price as u128)
        .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
        .checked_div(MICRO_LAMPORTS_PER_LAMPORT as u128)
        .and_then(|fee| u64::try_from(fee).ok())
        .unwrap_or(u64::MAX)
    }

    pub fn update(&mut self, slot: Slot, cu_cost: u64, cu_cost_limit: u64) {
        let prev_cu_utilization_ema = self.cu_utilization.get_ema();
        let prev_cu_utilization_stddev = self.cu_utilization.get_stddev();
        let prev_cu_price = self.cu_price;
        let this_cu_utilization = cu_cost * 100 / cu_cost_limit;

        self.slot = slot;
        self.cu_utilization.aggregate(this_cu_utilization);
        let post_cu_utilization_ema = self.cu_utilization.get_ema();
        let post_cu_utilization_stddev = self.cu_utilization.get_stddev();

        if post_cu_utilization_ema > CU_UTILIZATION_UPPER_BOUND {
            self.cu_price = PRICE_CHANGE_SCALE
                .saturating_add(PRICE_CHANGE_RATE)
                .saturating_mul(self.cu_price)
                .saturating_div(PRICE_CHANGE_SCALE);
        } else if post_cu_utilization_ema < CU_UTILIZATION_LOWER_BOUND {
            self.cu_price = PRICE_CHANGE_SCALE
                .saturating_sub(PRICE_CHANGE_RATE)
                .saturating_mul(self.cu_price)
                .saturating_div(PRICE_CHANGE_SCALE);
        } else {
            // mean reversion, if ema is within "band", cu-price should revert back to "normal",
            // some kind of elasticity to cu_price
            /* letit be free while testing
            match self.cu_price.cmp(&NORMAL_CU_PRICE) {
                Ordering::Equal => (),
                Ordering::Greater => {
                    self.cu_price = PRICE_CHANGE_SCALE
                        .saturating_sub(PRICE_CHANGE_RATE)
                        .saturating_mul(self.cu_price)
                        .saturating_div(PRICE_CHANGE_SCALE)
                        .max(NORMAL_CU_PRICE);
                }
                Ordering::Less => {
                    self.cu_price = PRICE_CHANGE_SCALE
                        .saturating_add(PRICE_CHANGE_RATE)
                        .saturating_mul(self.cu_price)
                        .saturating_div(PRICE_CHANGE_SCALE)
                        .min(NORMAL_CU_PRICE);
                }
            }
            // */
        }

        println!("=== slot {} cu_cost {} cu_cost_limit {} this_cu_util {} \
                 prev_cu_util_ems {} prev_cu_util_stddev {} \
                 post_cu_util_ema {} post_cu_util_stddev {} \
                 prev_cu_price {} post_cu_price {}",
                 self.slot,
                 cu_cost,
                 cu_cost_limit,
                 this_cu_utilization,
                 prev_cu_utilization_ema,
                 prev_cu_utilization_stddev,
                 post_cu_utilization_ema,
                 post_cu_utilization_stddev,
                 prev_cu_price,
                 self.cu_price,
                 );
    }
}

