use {solana_sdk::clock::Slot, std::time::SystemTime};

#[derive(Default)]
pub struct LeaderSlotsTracker {
    pub ranges: Vec<SlotEndEventRange>,
}

impl LeaderSlotsTracker {
    /// Returns true if a new slot range was started.
    pub fn insert_slot(&mut self, timestamp: SystemTime, slot: Slot) -> bool {
        let slot_end_event = SlotEndEvent { slot, timestamp };
        match self.ranges.last_mut() {
            Some(most_recent_slot_end_event) if slot == most_recent_slot_end_event.end.slot + 1 => {
                most_recent_slot_end_event.end = slot_end_event;
                false
            }
            _ => {
                self.ranges.push(SlotEndEventRange {
                    start: slot_end_event,
                    end: slot_end_event,
                });
                true
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct SlotEndEvent {
    pub slot: Slot,
    pub timestamp: SystemTime,
}

#[derive(Copy, Clone)]
pub struct SlotEndEventRange {
    pub start: SlotEndEvent,
    pub end: SlotEndEvent,
}
