use {
    crossbeam_channel::{Receiver, Sender},
    solana_sdk::clock::Slot,
};

/// PohTimingPoint. Each TimingPoint is annotated a timestamp in milliseconds.
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
    /// timing events
    pub timing_point: PohTimingPoint,
}

/// Receiver of SlotPohTimingInfo
pub type PohTimingReceiver = Receiver<SlotPohTimingInfo>;

/// Sender of SlotPohTimingInfo
pub type PohTimingSender = Sender<SlotPohTimingInfo>;
