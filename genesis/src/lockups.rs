//! investor stakes generator
use solana_sdk::clock::Epoch;

#[derive(Debug, Default)]
pub struct Lockups {
    /// where in iteration progress I am
    increment: usize,
    /// number of increments post cliff
    increments: usize,
    /// fractionage offered to f() last time
    prev_fraction: f64,
    /// portion of total available at cliff
    cliff_fraction: f64,
    /// time of cliff, in epochs
    cliff_epoch: Epoch,
    /// number of post-cliff events
    increment_fraction: f64,
    /// time between each post-cliff event
    increment_epochs: Epoch,
}

impl Lockups {
    pub fn new(
        cliff_fraction: f64,   // first cliff fraction
        cliff_year: Epoch,     // first cliff epoch
        increments: usize,     //  number of follow-on increments
        increment_year: Epoch, // epochs between each following increment
    ) -> Self {
        let increment_fraction = (1.0 - cliff_fraction) / increments as f64;

        Self {
            prev_fraction: 0.0,
            increment: 0,
            increments,
            cliff_fraction,
            cliff_epoch,
            increment_fraction,
            increment_epochs,
        }
    }
}

impl Iterator for Lockups {
    type Item = LockupIncrement;

    fn next(&mut self) -> Option<Self::Item> {
        if 0 == self.increment {
            self.prev_fraction = self.cliff_fraction;
            self.increment += 1;

            Some(LockupIncrement {
                prev_fraction: 0.0,
                fraction: self.cliff_fraction,
                epoch: self.cliff_epoch,
            })
        } else if self.increment <= self.increments {
            // save these off
            let (prev_fraction, increment) = (self.prev_fraction, self.increment);

            // move forward, tortured-looking math comes from wanting to reach 1.0 by the last
            //  increment
            self.prev_fraction =
                1.0 - (self.increments - increment) as f64 * self.increment_fraction;
            self.increment += 1;

            Some(LockupIncrement {
                prev_fraction,
                fraction: self.prev_fraction,
                epoch: self.cliff_epoch + increment as u64 * self.increment_epochs,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
pub struct LockupIncrement {
    /// the fraction of the lockup that expired in the previous increment
    pub prev_fraction: f64,
    /// the fraction of the lockup that expires in this increment
    pub fraction: f64,
    /// the epoch at which this increment expires
    pub epoch: Epoch,
}

impl LockupIncrement {
    pub fn lamports(&self, total_lamports: u64) -> u64 {
        (self.fraction * total_lamports as f64) as u64
            - (self.prev_fraction * total_lamports as f64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPOCHS_PER_MONTH: Epoch = 2;

    #[test]
    fn test_make_lockups() {
        // this number made up, to induce rounding errors
        let total_lamports: u64 = 1725987234408924;

        assert_eq!(
            Lockups::new(0.20, 6 * EPOCHS_PER_MONTH, 24, EPOCHS_PER_MONTH)
                .map(|increment| {
                    dbg!(&increment);
                    increment.lamports(total_lamports)
                })
                .sum::<u64>(),
            tokens
        );
    }
}
