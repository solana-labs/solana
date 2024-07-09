use {std::time::Duration, tokio::time::Instant};

#[derive(Debug)]
pub struct RateLimiter {
    /// count of requests in an interval
    pub(crate) count: u64,

    /// Rate limit start time
    start_instant: Instant,
    interval: Duration,
    limit: u64,
}

/// A naive rate limiter, to be replaced by using governor which has more even
/// distribution of requests passing through using GCRA algorithm.
impl RateLimiter {
    pub fn new(limit: u64, interval: Duration) -> Self {
        Self {
            count: 0,
            start_instant: Instant::now(),
            interval,
            limit,
        }
    }

    /// Reset the counter and start instant if needed.
    pub fn reset_params_if_needed(&mut self) {
        if Instant::now().duration_since(self.start_instant) > self.interval {
            self.start_instant = Instant::now();
            self.count = 0;
        }
    }

    /// Check if a single request should be allowed to pass through the rate limiter
    /// When it is allowed, the rate limiter state is updated to reflect it has been
    /// allowed. For a unique request, the caller should call it only once when it is allowed.
    pub fn check_and_update(&mut self) -> bool {
        self.reset_params_if_needed();
        if self.count >= self.limit {
            return false;
        }

        self.count = self.count.saturating_add(1);
        true
    }

    /// Return the start instant for the current rate-limiting interval.
    pub fn start_instant(&self) -> &Instant {
        &self.start_instant
    }
}

#[cfg(test)]
pub mod test {
    use {super::*, tokio::time::sleep};

    #[tokio::test]
    async fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(2, Duration::from_millis(100));
        assert!(limiter.check_and_update());
        assert!(limiter.check_and_update());
        assert!(!limiter.check_and_update());
        let instant1 = *limiter.start_instant();

        // sleep 150 ms, the rate-limiting parameters should have been reset.
        sleep(Duration::from_millis(150)).await;
        assert!(limiter.check_and_update());
        assert!(limiter.check_and_update());
        assert!(!limiter.check_and_update());

        let instant2 = *limiter.start_instant();
        assert!(instant2 > instant1);
    }
}
