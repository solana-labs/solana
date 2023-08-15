use {
    crate::{leader_slots_tracker::LeaderSlotsTracker, process::process_event_files},
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{BankingPacketBatch, ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_sdk::clock::Slot,
    std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    },
};

pub fn do_leader_priority_heatmap(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut tracker = LeaderPriorityTracker::default();
    process_event_files(event_file_paths, &mut |event| tracker.handle_event(event))?;
    tracker.report();
    Ok(())
}

/// For each contiguous group of leader slots, collect metrics for priority and time-offset.
/// Time-offset is measured from the end of the first slot.
#[derive(Default)]
pub struct LeaderPriorityTracker {
    leader_slots_tracker: LeaderSlotsTracker,
    current_collection: Vec<PriorityTimeData>,
    collected_data: Vec<LeaderPriorityData>,
}

impl LeaderPriorityTracker {
    fn handle_event(&mut self, TimedTracedEvent(timestamp, event): TimedTracedEvent) {
        match event {
            TracedEvent::PacketBatch(label, banking_packet_batch) => {
                self.handle_packets(timestamp, label, banking_packet_batch)
            }
            TracedEvent::BlockAndBankHash(slot, _, _) => self.handle_slot_end(timestamp, slot),
        }
    }

    fn handle_packets(
        &mut self,
        timestamp: SystemTime,
        label: ChannelLabel,
        banking_packet_batch: BankingPacketBatch,
    ) {
        if !matches!(label, ChannelLabel::NonVote) {
            return;
        }
        if banking_packet_batch.0.is_empty() {
            return;
        }

        let priority_time_data = banking_packet_batch
            .0
            .iter()
            .flatten()
            .cloned()
            .filter_map(|p| ImmutableDeserializedPacket::new(p).ok())
            .map(|p| PriorityTimeData {
                priority: p.priority(),
                timestamp,
            });
        self.current_collection.extend(priority_time_data);
    }

    fn handle_slot_end(&mut self, timestamp: SystemTime, slot: Slot) {
        let new_range = self.leader_slots_tracker.insert_slot(timestamp, slot);

        // If there is only one range, no work to be done. Just push the data into it.
        if self.collected_data.is_empty() {
            let range_index = self.leader_slots_tracker.ranges.len() - 1;
            let slot_range_start_ns = system_time_to_timestamp_ns(
                self.leader_slots_tracker.ranges[range_index]
                    .start
                    .timestamp,
            );
            // Drain current collection and calculate offset from the beginnning of the last range.
            let current_collection = core::mem::take(&mut self.current_collection);
            let packet_priority_offsets = current_collection
                .into_iter()
                .map(|d| {
                    let timestamp_ns = system_time_to_timestamp_ns(d.timestamp);
                    let offset_ns = (timestamp_ns - slot_range_start_ns).try_into().unwrap();
                    PacketPriorityOffset {
                        priority: d.priority,
                        offset_ns,
                    }
                })
                .collect();

            self.collected_data.push(LeaderPriorityData {
                data: packet_priority_offsets,
            });
        } else if new_range {
            self.collected_data.push(LeaderPriorityData::default());
            let range_index_1 = self.leader_slots_tracker.ranges.len() - 2;
            let range_index_2 = self.leader_slots_tracker.ranges.len() - 1;
            let range_1 = &self.leader_slots_tracker.ranges[range_index_1];
            let range_2 = &self.leader_slots_tracker.ranges[range_index_2];
            let range_1_start_ns = system_time_to_timestamp_ns(range_1.start.timestamp);
            let range_2_start_ns = system_time_to_timestamp_ns(range_2.start.timestamp);

            // Drain current collection and calculate offset from the beginnning of the last two ranges.
            let current_collection = core::mem::take(&mut self.current_collection);
            let packet_priority_offsets = current_collection.into_iter().map(|d| {
                let timestamp_ns = system_time_to_timestamp_ns(d.timestamp);
                let range_1_offset_ns = (timestamp_ns - range_1_start_ns).try_into().unwrap();
                let range_2_offset_ns = (timestamp_ns - range_2_start_ns).try_into().unwrap();
                (d.priority, range_1_offset_ns, range_2_offset_ns)
            });

            for (priority, range_1_offset_ns, range_2_offset_ns) in packet_priority_offsets {
                let abs_1 = i64::abs(range_1_offset_ns);
                let abs_2 = i64::abs(range_2_offset_ns);
                const THRESHOLD_NS: i64 = 30 * 1000 * 1000 * 1000; // 30 seconds
                if abs_1 > THRESHOLD_NS && abs_2 > THRESHOLD_NS {
                    // Both ranges are too far away from the packet timestamp. Discard.
                    continue;
                }
                if abs_1 > abs_2 {
                    self.collected_data[range_index_2]
                        .data
                        .push(PacketPriorityOffset {
                            priority,
                            offset_ns: range_2_offset_ns,
                        });
                } else {
                    self.collected_data[range_index_1]
                        .data
                        .push(PacketPriorityOffset {
                            priority,
                            offset_ns: range_1_offset_ns,
                        });
                }
            }
        }
    }

    fn report(&self) {
        for (slot_range, data) in self
            .leader_slots_tracker
            .ranges
            .iter()
            .zip(self.collected_data.iter())
        {
            let slot_start = slot_range.start.slot;
            let slot_end = slot_range.end.slot;
            let priority_stats = get_stats(data.data.iter().map(|d| d.priority as f64));
            let offset_stats = get_stats(data.data.iter().map(|d| d.offset_ns as f64));
            let prioritized_offset_stats = get_stats(
                data.data
                    .iter()
                    .filter_map(|d| (d.priority != 0).then_some(d.offset_ns as f64)),
            );
            let unprioritized_offset_stats = get_stats(
                data.data
                    .iter()
                    .filter_map(|d| (d.priority == 0).then_some(d.offset_ns as f64)),
            );

            println!("{slot_start} - {slot_end}:");
            println!("    Priority: {priority_stats}");
            println!("    OffsetNs: {offset_stats}");
            println!("    Prioritized-OffsetNs: {prioritized_offset_stats}");
            println!("    UnPrioritized-OffsetNs: {unprioritized_offset_stats}");
        }
    }
}

struct Stats {
    min: f64,
    max: f64,
    average: f64,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{min: {}, max: {}, average: {}}}",
            self.min, self.max, self.average
        )
    }
}

fn get_stats(iter: impl Iterator<Item = f64>) -> Stats {
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    let mut sum = 0.0;
    let mut count = 0;
    for item in iter.map(|i| i.try_into().unwrap()) {
        min = min.min(item);
        max = max.max(item);
        sum += item;
        count += 1;
    }

    let average = sum / count as f64;
    Stats { min, max, average }
}

/// Aggregate priority and timestamp info for a contiguous group of leader slots.
struct PriorityTimeData {
    priority: u64,
    timestamp: SystemTime,
}

#[derive(Default)]
struct LeaderPriorityData {
    data: Vec<PacketPriorityOffset>,
}

struct PacketPriorityOffset {
    /// The priority of the packet.
    priority: u64,
    /// Offset in ns from the end of the first slot.
    offset_ns: i64,
}

fn system_time_to_timestamp_ns(timestamp: SystemTime) -> i128 {
    i128::try_from(timestamp.duration_since(UNIX_EPOCH).unwrap().as_nanos()).unwrap()
}
