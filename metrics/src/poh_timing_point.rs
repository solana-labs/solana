use {
    crossbeam_channel::{Receiver, Sender},
    solana_sdk::clock::Slot,
};

/// PohTimingPoint. Each TimingPoint is annotated with a timestamp in milliseconds.
#[derive(Debug, Clone)]
pub enum PohTimingPoint {
    PohSlotStart(u64),
    PohSlotEnd(u64),
    FullSlotReceived(u64),
}

/// SlotPohTimingInfo
#[derive(Clone, Debug)]
pub struct SlotPohTimingInfo {
    /// current slot
    pub slot: Slot,
    /// root slot
    pub root_slot: Option<Slot>,
    /// timing event
    pub timing_point: PohTimingPoint,
}

#[macro_export]
macro_rules! create_slot_poh_time_point {
    ($slot:expr, $timestamp:expr) => {
        $crate::poh_timing_point::SlotPohTimingInfo {
            slot: $slot,
            root_slot: None,
            timing_point: $timestamp,
        }
    };

    ($slot:expr, $root_slot:expr, $timestamp:expr ) => {
        $crate::poh_timing_point::SlotPohTimingInfo {
            slot: $slot,
            root_slot: Some($root_slot),
            timing_point: $timestamp,
        }
    };
}

#[macro_export]
macro_rules! create_slot_poh_start_time_point {
    ($slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $crate::poh_timing_point::PohTimingPoint::PohSlotStart($timestamp)
        )
    };
    ($slot:expr, $root_slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $root_slot,
            $crate::poh_timing_point::PohTimingPoint::PohSlotStart($timestamp)
        )
    };
}

#[macro_export]
macro_rules! create_slot_poh_end_time_point {
    ($slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $crate::poh_timing_point::PohTimingPoint::PohSlotEnd($timestamp)
        )
    };
    ($slot:expr, $root_slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $root_slot,
            $crate::poh_timing_point::PohTimingPoint::PohSlotEnd($timestamp)
        )
    };
}

#[macro_export]
macro_rules! create_slot_poh_full_time_point {
    ($slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $crate::poh_timing_point::PohTimingPoint::FullSlotReceived($timestamp)
        )
    };
    ($slot:expr, $root_slot:expr, $timestamp:expr) => {
        $crate::create_slot_poh_time_point!(
            $slot,
            $root_slot,
            $crate::poh_timing_point::PohTimingPoint::FullSlotReceived($timestamp)
        )
    };
}

/// Receiver of SlotPohTimingInfo from the channel
pub type PohTimingReceiver = Receiver<SlotPohTimingInfo>;

/// Sender of SlotPohTimingInfo to the channel
pub type PohTimingSender = Sender<SlotPohTimingInfo>;

#[cfg(test)]
mod test {
    #[test]
    fn test_poh_timing_point() {
        let _ = create_slot_poh_start_time_point!(100, 101, 100);
        let _ = create_slot_poh_start_time_point!(100, 100);

        let _ = create_slot_poh_end_time_point!(100, 101, 100);
        let _ = create_slot_poh_end_time_point!(100, 100);

        let _ = create_slot_poh_full_time_point!(100, 101, 100);
        let _ = create_slot_poh_full_time_point!(100, 100);
    }
}
