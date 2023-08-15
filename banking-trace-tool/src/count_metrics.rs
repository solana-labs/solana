use {
    crate::process::process_event_files,
    histogram::Histogram,
    solana_core::banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    std::path::PathBuf,
};

pub fn do_count_metrics(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut collector = CountMetricsCollector::default();
    let mut metric_collector = |event| collector.handle_event(event);
    process_event_files(event_file_paths, &mut metric_collector)?;
    collector.report();

    Ok(())
}

#[derive(Default)]
struct CountMetricsCollector {
    total_batches: usize,
    total_packets: usize,
    batch_count_histogram: Histogram,
    non_zero_batch_count_histogram: Histogram,
    batch_length_histogram: Histogram,
    packet_count_histogram: Histogram,
}

impl CountMetricsCollector {
    pub fn handle_event(&mut self, TimedTracedEvent(_, event): TimedTracedEvent) {
        let TracedEvent::PacketBatch(label, banking_packet_batch) = event else {
            return;
        };

        if !matches!(label, ChannelLabel::NonVote) {
            return;
        }

        let packet_batches = &banking_packet_batch.0;

        let num_batches = packet_batches.len();
        self.batch_count_histogram
            .increment(num_batches as u64)
            .unwrap();
        if num_batches > 0 {
            self.non_zero_batch_count_histogram
                .increment(num_batches as u64)
                .unwrap();
            let mut num_packets = 0;
            for batch in packet_batches {
                num_packets += batch.len();
                self.batch_length_histogram
                    .increment(batch.len() as u64)
                    .unwrap();
            }
            self.packet_count_histogram
                .increment(num_packets as u64)
                .unwrap();

            self.total_batches += num_batches;
            self.total_packets += num_packets;
        }
    }

    fn report(&self) {
        println!("total_batches: {}", self.total_batches);
        println!("total_packets: {}", self.total_packets);
        pretty_print_histogram("batch_count", &self.batch_count_histogram);
        pretty_print_histogram("non-zero batch_count", &self.non_zero_batch_count_histogram);
        pretty_print_histogram("batch_length", &self.batch_length_histogram);
        pretty_print_histogram("packet_count", &self.packet_count_histogram);
    }
}

fn pretty_print_histogram(name: &str, histogram: &Histogram) {
    print!("{name}: [");

    const PERCENTILES: &[f64] = &[5.0, 10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 99.9];
    for percentile in PERCENTILES.iter().copied() {
        print!(
            "{}: {}, ",
            percentile,
            histogram.percentile(percentile).unwrap()
        );
    }
    println!("]");
}
