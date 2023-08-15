use {
    crate::{
        leader_slots_tracker::{LeaderSlotsTracker, SlotEndEventRange},
        process::process_event_files,
    },
    chrono::{DateTime, Utc},
    solana_core::banking_trace::{TimedTracedEvent, TracedEvent},
    std::path::PathBuf,
};

pub fn do_get_slot_ranges(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut collector = SlotRanges::default();
    process_event_files(event_file_paths, &mut |event| collector.handle_event(event))?;
    collector.report();
    Ok(())
}

#[derive(Default)]
struct SlotRanges {
    collector: LeaderSlotsTracker,
}

impl SlotRanges {
    pub fn handle_event(&mut self, TimedTracedEvent(timestamp, event): TimedTracedEvent) {
        if let TracedEvent::BlockAndBankHash(slot, _, _) = event {
            self.collector.insert_slot(timestamp, slot);
        }
    }

    fn report(&self) {
        println!("[");
        for SlotEndEventRange { start, end } in self.collector.ranges.iter() {
            println!(
                "{} - {} ({} - {})",
                start.slot,
                end.slot,
                DateTime::<Utc>::from(start.timestamp),
                DateTime::<Utc>::from(end.timestamp)
            );
        }
        println!("]");
    }
}
