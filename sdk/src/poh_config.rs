use crate::timing::DEFAULT_TICKS_PER_SECOND;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PohConfig {
    /// The target tick rate of the cluster.
    pub target_tick_duration: Duration,

    /// How many hashes to roll before emitting the next tick entry.
    /// None enables "Low power mode", which implies:
    /// * sleep for `target_tick_duration` instead of hashing
    /// * the number of hashes per tick will be variable
    pub hashes_per_tick: Option<u64>,
}

impl PohConfig {
    pub fn new_sleep(target_tick_duration: Duration) -> Self {
        Self {
            target_tick_duration,
            hashes_per_tick: None,
        }
    }
}

impl Default for PohConfig {
    fn default() -> Self {
        Self::new_sleep(Duration::from_millis(1000 / DEFAULT_TICKS_PER_SECOND))
    }
}
