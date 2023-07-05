use std::time::Duration;

enum PacketThresholdUpdate {
    Increase,
    Decrease,
}

impl PacketThresholdUpdate {
    const PERCENTAGE: usize = 90;

    fn calculate(&self, current: usize) -> usize {
        match *self {
            PacketThresholdUpdate::Increase => {
                current.saturating_mul(100).saturating_div(Self::PERCENTAGE)
            }
            PacketThresholdUpdate::Decrease => {
                current.saturating_mul(Self::PERCENTAGE).saturating_div(100)
            }
        }
    }
}

#[derive(Debug)]
pub struct DynamicPacketToProcessThreshold {
    max_packets: usize,
}

impl Default for DynamicPacketToProcessThreshold {
    fn default() -> Self {
        Self {
            max_packets: Self::DEFAULT_MAX_PACKETS,
        }
    }
}

impl DynamicPacketToProcessThreshold {
    const DEFAULT_MAX_PACKETS: usize = 1024;
    const TIME_THRESHOLD: Duration = Duration::from_secs(1);

    pub fn update(&mut self, total_packets: usize, compute_time: Duration) {
        if total_packets >= self.max_packets {
            let threshold_update = if compute_time > Self::TIME_THRESHOLD {
                PacketThresholdUpdate::Decrease
            } else {
                PacketThresholdUpdate::Increase
            };
            self.max_packets = threshold_update.calculate(self.max_packets);
        }
    }

    pub fn should_drop(&self, total: usize) -> bool {
        total >= self.max_packets
    }
}

#[cfg(test)]
mod test {
    use {super::DynamicPacketToProcessThreshold, std::time::Duration};

    #[test]
    fn test_dynamic_packet_threshold() {
        let mut threshold = DynamicPacketToProcessThreshold::default();
        assert_eq!(
            threshold.max_packets,
            DynamicPacketToProcessThreshold::DEFAULT_MAX_PACKETS
        );

        assert!(!threshold.should_drop(10));
        assert!(threshold.should_drop(2000));

        let old = threshold.max_packets;

        // Increase
        let total = 2000;
        let compute_time = Duration::from_millis(500);
        threshold.update(total, compute_time);
        assert!(threshold.max_packets > old);

        // Decrease
        let compute_time = Duration::from_millis(2000);
        threshold.update(total, compute_time);
        assert_eq!(threshold.max_packets, old - 1); // due to rounding error, there is a difference of 1
    }
}
