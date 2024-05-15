use {
    crate::nonblocking::{keyed_rate_limiter::KeyedRateLimiter, rate_limiter::RateLimiter},
    std::{net::IpAddr, time::Duration},
};

pub struct ConnectionRateLimiter {
    limiter: KeyedRateLimiter<IpAddr>,
}

impl ConnectionRateLimiter {
    /// Create a new rate limiter per IpAddr. The rate is specified as the count per minute to allow for
    /// less frequent connections.
    pub fn new(limit_per_minute: u64) -> Self {
        Self {
            limiter: KeyedRateLimiter::new(limit_per_minute, Duration::from_secs(60)),
        }
    }

    /// Check if the connection from the said `ip` is allowed.
    pub fn is_allowed(&self, ip: &IpAddr) -> bool {
        // Acquire a permit from the rate limiter for the given IP address
        if self.limiter.check_and_update(*ip) {
            debug!("Request from IP {:?} allowed", ip);
            true // Request allowed
        } else {
            debug!("Request from IP {:?} blocked", ip);
            false // Request blocked
        }
    }

    /// retain only keys whose throttle start date is within the throttle interval.
    /// Otherwise drop them as inactive
    pub fn retain_recent(&self) {
        self.limiter.retain_recent()
    }

    /// Returns the number of "live" keys in the rate limiter.
    pub fn len(&self) -> usize {
        self.limiter.len()
    }

    /// Returns `true` if the rate limiter has no keys in it.
    pub fn is_empty(&self) -> bool {
        self.limiter.is_empty()
    }
}

/// Connection rate limiter for enforcing connection rates from
/// all clients.
pub struct TotalConnectionRateLimiter {
    limiter: RateLimiter,
}

impl TotalConnectionRateLimiter {
    /// Create a new rate limiter. The rate is specified as the count per second.
    pub fn new(limit_per_second: u64) -> Self {
        Self {
            limiter: RateLimiter::new(limit_per_second, Duration::from_secs(1)),
        }
    }

    /// Check if a connection is allowed.
    pub fn is_allowed(&mut self) -> bool {
        self.limiter.check_and_update()
    }
}

#[cfg(test)]
pub mod test {
    use {super::*, std::net::Ipv4Addr};

    #[tokio::test]
    async fn test_total_connection_rate_limiter() {
        let mut limiter = TotalConnectionRateLimiter::new(2);
        assert!(limiter.is_allowed());
        assert!(limiter.is_allowed());
        assert!(!limiter.is_allowed());
    }

    #[tokio::test]
    async fn test_connection_rate_limiter() {
        let limiter = ConnectionRateLimiter::new(4);
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        assert!(limiter.is_allowed(&ip1));
        assert!(limiter.is_allowed(&ip1));
        assert!(limiter.is_allowed(&ip1));
        assert!(limiter.is_allowed(&ip1));
        assert!(!limiter.is_allowed(&ip1));

        assert!(limiter.len() == 1);
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        assert!(limiter.is_allowed(&ip2));
        assert!(limiter.len() == 2);
        assert!(limiter.is_allowed(&ip2));
        assert!(limiter.is_allowed(&ip2));
        assert!(limiter.is_allowed(&ip2));
        assert!(!limiter.is_allowed(&ip2));
    }
}
