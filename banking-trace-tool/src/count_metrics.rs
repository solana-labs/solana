use {
    crate::process::process_event_files,
    histogram::Histogram,
    solana_core::banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    std::path::PathBuf,
};

pub fn do_count_metrics(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut total_batches = 0;
    let mut total_packets = 0;
    let mut batch_count_histogram = Histogram::new();
    let mut non_zero_batch_count_histogram = Histogram::new();
    let mut batch_length_histogram = Histogram::new();
    let mut packet_count_histogram = Histogram::new();

    let mut metric_collector = |TimedTracedEvent(_, event): TimedTracedEvent| {
        let TracedEvent::PacketBatch(label, banking_packet_batch) = event else {
            return;
        };

        if !matches!(label, ChannelLabel::NonVote) {
            return;
        }

        let packet_batches = &banking_packet_batch.0;

        let num_batches = packet_batches.len();
        batch_count_histogram.increment(num_batches as u64).unwrap();
        if num_batches > 0 {
            non_zero_batch_count_histogram
                .increment(num_batches as u64)
                .unwrap();
            let mut num_packets = 0;
            for batch in packet_batches {
                num_packets += batch.len();
                batch_length_histogram
                    .increment(batch.len() as u64)
                    .unwrap();
            }
            packet_count_histogram
                .increment(num_packets as u64)
                .unwrap();

            total_batches += num_batches;
            total_packets += num_packets;
        }
    };

    process_event_files(event_file_paths, &mut metric_collector)?;

    println!("total_batches: {}", total_batches);
    println!("total_packets: {}", total_packets);

    pretty_print_histogram("batch_count", &batch_count_histogram);
    pretty_print_histogram("non-zero batch_count", &non_zero_batch_count_histogram);
    pretty_print_histogram("batch_length", &batch_length_histogram);
    pretty_print_histogram("packet_count", &packet_count_histogram);

    Ok(())
}

fn pretty_print_histogram(name: &str, histogram: &Histogram) {
    print!("{name}: [");

    const PERCENTILES: &[f64] = &[5.0, 10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 99.9];
    for percentile in PERCENTILES.into_iter().copied() {
        print!(
            "{}: {}, ",
            percentile,
            histogram.percentile(percentile).unwrap()
        );
    }
    println!("]");
}
