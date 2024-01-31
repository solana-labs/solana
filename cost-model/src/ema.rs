// N is number of datapoints used in exponential weighted moving average calculoation.
// default to 16 blocks
const N: i128 = 16;
// The EMA_ALPHA represents the degree of weighting decrease in EMA,
// a constant smoothing factor between 0 and 1. A higher alpha
// discounts older observations faster.
// Estimate it to 0.117 by `2/(N+1)` if N=16
const EMA_SCALE: i128 = 1000;
const EMA_ALPHA: i128 = 2 * EMA_SCALE / (N + 1);

// exponential moving average algorithm
// https://en.wikipedia.org/wiki/Moving_average#Exponentially_weighted_moving_variance_and_standard_deviation
#[derive(Clone, Debug, Default)]
pub struct AggregatedVarianceStats {
    ema: u64,
    ema_var: u64,
}

impl AggregatedVarianceStats {
    pub fn new_with_initial_ema(value: u64) -> Self {
        Self {
            ema: value,
            ema_var: 0u64,
        }
    }

    pub fn get_ema(&self) -> u64 {
        self.ema
    }

    pub fn get_stddev(&self) -> u64 {
        (self.ema_var as f64).sqrt().ceil() as u64
    }

    fn aggregate_ema(&mut self, theta: i128) {
        self.ema = u64::try_from(
            i128::from(self.ema)
                .saturating_mul(EMA_SCALE)
                .saturating_add(theta.saturating_mul(EMA_ALPHA))
                .saturating_div(EMA_SCALE),
        )
        .ok()
        .unwrap_or(self.ema);
    }

    fn aggregate_variance(&mut self, theta: i128) {
        self.ema_var = u64::try_from(
            i128::from(self.ema_var)
                .saturating_mul(EMA_SCALE)
                .saturating_add(theta.saturating_pow(2).saturating_mul(EMA_ALPHA))
                .saturating_mul(EMA_SCALE - EMA_ALPHA)
                .saturating_div(EMA_SCALE.saturating_pow(2)),
        )
        .ok()
        .unwrap_or(self.ema_var);
    }

    pub fn aggregate(&mut self, new_value: u64) {
        if self.get_ema() == 0 {
            // first datapoint
            self.ema = new_value;
            self.ema_var = 0u64;
        } else {
            let theta = i128::from(new_value) - i128::from(self.get_ema());
            self.aggregate_ema(theta);
            self.aggregate_variance(theta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_variance_stats() {
        solana_logger::setup();
        let cost: u64 = u64::MAX;

        // set initial value at MAX
        let mut aggregated_variance_stats = AggregatedVarianceStats {
            ema: cost,
            ema_var: 0,
        };
        assert_eq!(cost, aggregated_variance_stats.get_ema());
        assert_eq!(0, aggregated_variance_stats.get_stddev());

        // expected variance
        let expected_var = 33u64;
        // insert a positive theta, ema will be saturated, hence remain at its MAX value
        {
            let theta = 100i128;
            aggregated_variance_stats.aggregate_ema(theta);
            aggregated_variance_stats.aggregate_variance(theta);
            assert_eq!(cost, aggregated_variance_stats.get_ema());
            assert_eq!(expected_var, aggregated_variance_stats.get_stddev());
        }

        // insert a negative theta, would reduce ema, increase variance
        {
            let theta = -100i128;
            aggregated_variance_stats.aggregate_ema(theta);
            aggregated_variance_stats.aggregate_variance(theta);
            assert!(cost > aggregated_variance_stats.get_ema());
            assert!(expected_var < aggregated_variance_stats.get_stddev());
        }
    }
}
