use {
    crate::{leader_slots_tracker::LeaderSlotsTracker, process::process_event_files},
    itertools::{Itertools, MinMaxResult},
    plotters::prelude::*,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{BankingPacketBatch, ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_sdk::clock::Slot,
    std::{
        cmp::Ordering,
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
        const THRESHOLD_NS: i64 = 8400 * 1000 * 1000; // 8.4 seconds

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
                .filter_map(|d| {
                    let timestamp_ns = system_time_to_timestamp_ns(d.timestamp);
                    let offset_ns = (timestamp_ns - slot_range_start_ns).try_into().unwrap();

                    (i64::abs(offset_ns) <= THRESHOLD_NS).then(|| PacketPriorityOffset {
                        priority: d.priority,
                        offset_ns,
                    })
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

                if abs_1 > abs_2 && abs_2 <= THRESHOLD_NS {
                    self.collected_data[range_index_2]
                        .data
                        .push(PacketPriorityOffset {
                            priority,
                            offset_ns: range_2_offset_ns,
                        });
                } else if abs_1 <= THRESHOLD_NS {
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
        const NANOS_PER_MILLI: f64 = 1000.0 * 1000.0;
        let mut overlayed_data = vec![];
        for (slot_range, data) in self
            .leader_slots_tracker
            .ranges
            .iter()
            .zip(self.collected_data.iter())
        {
            println!("{} - {}", slot_range.start.slot, slot_range.end.slot);

            let points = data
                .data
                .iter()
                .map(|d| (d.offset_ns as f64 / NANOS_PER_MILLI, d.priority as f64))
                .collect_vec();

            let filename = format!(
                "./heatmaps/{}-{}.png",
                slot_range.start.slot, slot_range.end.slot
            );
            generate_heatmap(&points, filename);
            overlayed_data.extend(points);
        }

        let filename = format!(
            "./heatmaps/overlay-{}-{}.png",
            self.leader_slots_tracker.ranges.first().unwrap().start.slot,
            self.leader_slots_tracker.ranges.last().unwrap().end.slot
        );
        generate_heatmap(&overlayed_data, filename);
    }
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

fn generate_heatmap(data: &[(f64, f64)], filename: impl AsRef<std::path::Path>) {
    let (x_buckets, y_buckets, counts) = bucket_counts(data);
    let (min_x, max_x) = (x_buckets.first().unwrap().0, x_buckets.last().unwrap().1);
    let (min_y, max_y) = (y_buckets.first().unwrap().0, y_buckets.last().unwrap().1);

    println!("  Total counts: {}", data.len());
    println!("  X: [{min_x}, {max_x}]");
    println!("  Y: [{min_y}, {max_y}]");

    let root = BitMapBackend::new(&filename, (1024, 1024)).into_drawing_area();
    root.fill(&WHITE).unwrap();

    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(75)
        .y_label_area_size(75)
        .build_cartesian_2d(min_x..max_x, min_y..max_y)
        .unwrap();

    chart
        .configure_mesh()
        .disable_mesh()
        .x_desc("Time Offset (ms)")
        .y_desc("Priority")
        .draw()
        .unwrap();

    let max_count = *counts.iter().flatten().max().unwrap() as f64;

    let color_interpolator = |f: f64| ViridisRGB::get_color(f);
    for (x_bucket, (x_lower, x_upper)) in x_buckets.iter().enumerate() {
        for (y_bucket, (y_lower, y_upper)) in y_buckets.iter().enumerate() {
            let count = counts[x_bucket][y_bucket];
            if count == 0 {
                continue;
            }
            let value = count as f64 / max_count;
            let color = color_interpolator(value);

            chart
                .draw_series([Rectangle::new(
                    [(*x_lower, *y_lower), (*x_upper, *y_upper)],
                    color.filled(),
                )])
                .unwrap();
        }
    }

    for x in [-400.0, 0.0, 400.0, 800.0, 1200.0] {
        let line_data = [(x, min_y), (x, max_y)];
        chart
            .draw_series(LineSeries::new(
                line_data.iter().map(|&(x, y)| (x, y)),
                BLACK,
            ))
            .unwrap();
    }

    root.present().unwrap();
}

/// Returns `(x_buckets, y_buckets, counts)`
fn bucket_counts(data: &[(f64, f64)]) -> (Vec<(f64, f64)>, Vec<(f64, f64)>, Vec<Vec<u64>>) {
    // Determine the buckets for the data.
    let (x_min, x_max) = minmax(data.iter().map(|&(x, _)| x));
    let (y_min, y_max) = minmax(data.iter().map(|&(_, y)| y));
    let x_buckets = linspace(x_min, x_max, 50);
    let y_buckets = linspace(y_min, y_max, 50);

    // Determine the counts for each bucket.
    let mut counts = vec![vec![0; y_buckets.len()]; x_buckets.len()];

    for &(x, y) in data {
        let x_bucket = x_buckets
            .binary_search_by(|&(x_lower, x_upper)| {
                if x >= x_lower && x <= x_upper {
                    Ordering::Equal
                } else if x < x_lower {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            })
            .unwrap();
        let y_bucket = y_buckets
            .binary_search_by(|&(y_lower, y_upper)| {
                if y >= y_lower && y <= y_upper {
                    Ordering::Equal
                } else if y < y_lower {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            })
            .unwrap();
        counts[x_bucket][y_bucket] += 1;
    }

    (x_buckets, y_buckets, counts)
}

/// Panicking minmax
fn minmax(it: impl Iterator<Item = f64>) -> (f64, f64) {
    let MinMaxResult::MinMax(min, max) = it.minmax() else {
        panic!("failed to get min/max of priorities");
    };

    (min, max)
}

fn linspace(min: f64, max: f64, n: usize) -> Vec<(f64, f64)> {
    let bucket_width = (max - min) / n as f64;

    let mut buckets = Vec::with_capacity(n - 1);
    let mut start = min;
    for i in 0..n {
        let end = if i == n - 1 {
            max
        } else {
            start + bucket_width
        };
        buckets.push((start, end));
        start = end;
    }

    buckets
}
