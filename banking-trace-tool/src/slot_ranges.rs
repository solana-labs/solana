use {
    crate::process::process_event_files,
    chrono::{DateTime, Utc},
    solana_core::banking_trace::{TimedTracedEvent, TracedEvent},
    solana_sdk::clock::Slot,
    std::{path::PathBuf, time::SystemTime},
};

pub fn do_get_slot_ranges(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut collector = SlotCollector::default();
    process_event_files(event_file_paths, &mut |event| collector.handle_event(event))?;
    collector.report();
    Ok(())
}

#[derive(Default)]
struct SlotCollector {
    ranges: Vec<SlotEndEventRange>,
}

impl SlotCollector {
    fn handle_event(&mut self, TimedTracedEvent(timestamp, event): TimedTracedEvent) {
        if let TracedEvent::BlockAndBankHash(slot, _, _) = event {
            let slot_end_event = SlotEndEvent { slot, timestamp };
            match self.ranges.last_mut() {
                Some(most_recent_slot_end_event)
                    if slot == most_recent_slot_end_event.end.slot + 1 =>
                {
                    most_recent_slot_end_event.end = slot_end_event;
                }
                _ => self.ranges.push(SlotEndEventRange {
                    start: slot_end_event,
                    end: slot_end_event,
                }),
            }
        }
    }

    fn report(&self) {
        println!("[");
        for SlotEndEventRange { start, end } in self.ranges.iter() {
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

#[derive(Copy, Clone)]
struct SlotEndEvent {
    slot: Slot,
    timestamp: SystemTime,
}

#[derive(Copy, Clone)]
struct SlotEndEventRange {
    start: SlotEndEvent,
    end: SlotEndEvent,
}
