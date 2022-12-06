//! A poh_timing_point module

use {
    crossbeam_channel::{Receiver, Sender},
    log::*,
    solana_sdk::clock::Slot,
    std::fmt,
};

/// Receiver of SlotPohTimingInfo from the channel
pub type PohTimingReceiver = Receiver<SlotPohTimingInfo>;

/// Sender of SlotPohTimingInfo to the channel
pub type PohTimingSender = Sender<SlotPohTimingInfo>;

/// PohTimingPoint. Each TimingPoint is annotated with a timestamp in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PohTimingPoint {
    PohSlotStart(u64),
    PohSlotEnd(u64),
    FullSlotReceived(u64),
}

impl fmt::Display for PohTimingPoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PohTimingPoint::PohSlotStart(t) => write!(f, "poh_start={t}"),
            PohTimingPoint::PohSlotEnd(t) => write!(f, "poh_end  ={t}"),
            PohTimingPoint::FullSlotReceived(t) => write!(f, "poh_full ={t}"),
        }
    }
}

/// SlotPohTimingInfo. This struct is sent to channel and received by
/// poh_timing_report service.
#[derive(Clone, Debug)]
pub struct SlotPohTimingInfo {
    /// current slot
    pub slot: Slot,
    /// root slot
    pub root_slot: Option<Slot>,
    /// timing event
    pub timing_point: PohTimingPoint,
}

impl fmt::Display for SlotPohTimingInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "PohTimingPoint: {}, slot={}, root_slot={}",
            self.timing_point,
            self.slot,
            self.root_slot.unwrap_or(0),
        )
    }
}

impl SlotPohTimingInfo {
    /// create slot start poh timing point
    pub fn new_slot_start_poh_time_point(
        slot: Slot,
        root_slot: Option<Slot>,
        timestamp: u64,
    ) -> SlotPohTimingInfo {
        SlotPohTimingInfo {
            slot,
            root_slot,
            timing_point: PohTimingPoint::PohSlotStart(timestamp),
        }
    }

    /// create slot end poh timing point
    pub fn new_slot_end_poh_time_point(
        slot: Slot,
        root_slot: Option<Slot>,
        timestamp: u64,
    ) -> SlotPohTimingInfo {
        SlotPohTimingInfo {
            slot,
            root_slot,
            timing_point: PohTimingPoint::PohSlotEnd(timestamp),
        }
    }

    /// create slot full poh timing point
    pub fn new_slot_full_poh_time_point(
        slot: Slot,
        root_slot: Option<Slot>,
        timestamp: u64,
    ) -> SlotPohTimingInfo {
        SlotPohTimingInfo {
            slot,
            root_slot,
            timing_point: PohTimingPoint::FullSlotReceived(timestamp),
        }
    }
}

/// send poh timing to channel
pub fn send_poh_timing_point(sender: &PohTimingSender, slot_timing: SlotPohTimingInfo) {
    trace!("{}", slot_timing);
    if let Err(e) = sender.try_send(slot_timing) {
        info!("failed to send slot poh timing {:?}", e);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_poh_timing_point() {
        // create slot start with root
        let p = SlotPohTimingInfo::new_slot_start_poh_time_point(100, Some(101), 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, Some(101));
        assert_eq!(p.timing_point, PohTimingPoint::PohSlotStart(100));
        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_start=100, slot=100, root_slot=101"
        );

        // create slot start without root
        let p = SlotPohTimingInfo::new_slot_start_poh_time_point(100, None, 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, None);
        assert_eq!(p.timing_point, PohTimingPoint::PohSlotStart(100));
        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_start=100, slot=100, root_slot=0"
        );

        // create slot end with root
        let p = SlotPohTimingInfo::new_slot_end_poh_time_point(100, Some(101), 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, Some(101));
        assert_eq!(p.timing_point, PohTimingPoint::PohSlotEnd(100));
        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_end  =100, slot=100, root_slot=101"
        );

        // create slot end without root
        let p = SlotPohTimingInfo::new_slot_end_poh_time_point(100, None, 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, None);
        assert_eq!(p.timing_point, PohTimingPoint::PohSlotEnd(100));
        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_end  =100, slot=100, root_slot=0"
        );

        // create slot full with root
        let p = SlotPohTimingInfo::new_slot_full_poh_time_point(100, Some(101), 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, Some(101));
        assert_eq!(p.timing_point, PohTimingPoint::FullSlotReceived(100));
        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_full =100, slot=100, root_slot=101"
        );

        // create slot full without root
        let p = SlotPohTimingInfo::new_slot_full_poh_time_point(100, None, 100);
        assert!(p.slot == 100);
        assert_eq!(p.root_slot, None);
        assert_eq!(p.timing_point, PohTimingPoint::FullSlotReceived(100));

        assert_eq!(
            format!("{p}"),
            "PohTimingPoint: poh_full =100, slot=100, root_slot=0"
        );
    }
}
