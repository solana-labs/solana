use {
    agave_transaction_view::bytes::{optimized_read_compressed_u16, read_compressed_u16},
    bincode::{serialize_into, DefaultOptions, Options},
    criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput},
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        short_vec::{decode_shortu16_len, ShortU16},
    },
};

fn setup() -> Vec<(u16, usize, Vec<u8>)> {
    let options = DefaultOptions::new().with_fixint_encoding(); // Ensure fixed-int encoding

    // Create a vector of all valid u16 values serialized into 16-byte buffers.
    let mut values = Vec::with_capacity(PACKET_DATA_SIZE);
    for value in 0..PACKET_DATA_SIZE as u16 {
        let short_u16 = ShortU16(value);
        let mut buffer = vec![0u8; 16];
        let serialized_len = options
            .serialized_size(&short_u16)
            .expect("Failed to get serialized size");
        serialize_into(&mut buffer[..], &short_u16).expect("Serialization failed");
        values.push((value, serialized_len as usize, buffer));
    }

    values
}

fn bench_u16_parsing(c: &mut Criterion) {
    let values_serialized_lengths_and_buffers = setup();
    let mut group = c.benchmark_group("compressed_u16_parsing");
    group.throughput(Throughput::Elements(
        values_serialized_lengths_and_buffers.len() as u64,
    ));

    // Benchmark the decode_shortu16_len function from `solana-sdk`
    group.bench_function("short_u16_decode", |c| {
        c.iter(|| {
            decode_shortu16_len_iter(&values_serialized_lengths_and_buffers);
        })
    });

    // Benchmark `read_compressed_u16`
    group.bench_function("read_compressed_u16", |c| {
        c.iter(|| {
            read_compressed_u16_iter(&values_serialized_lengths_and_buffers);
        })
    });

    group.bench_function("optimized_read_compressed_u16", |c| {
        c.iter(|| {
            optimized_read_compressed_u16_iter(&values_serialized_lengths_and_buffers);
        })
    });
}

fn decode_shortu16_len_iter(values_serialized_lengths_and_buffers: &[(u16, usize, Vec<u8>)]) {
    for (value, serialized_len, buffer) in values_serialized_lengths_and_buffers.iter() {
        let (read_value, bytes_read) = decode_shortu16_len(black_box(buffer)).unwrap();
        assert_eq!(read_value, *value as usize, "Value mismatch for: {}", value);
        assert_eq!(
            bytes_read, *serialized_len,
            "Offset mismatch for: {}",
            value
        );
    }
}

fn read_compressed_u16_iter(values_serialized_lengths_and_buffers: &[(u16, usize, Vec<u8>)]) {
    for (value, serialized_len, buffer) in values_serialized_lengths_and_buffers.iter() {
        let mut offset = 0;
        let read_value = read_compressed_u16(black_box(buffer), &mut offset).unwrap();
        assert_eq!(read_value, *value, "Value mismatch for: {}", value);
        assert_eq!(offset, *serialized_len, "Offset mismatch for: {}", value);
    }
}

fn optimized_read_compressed_u16_iter(
    values_serialized_lengths_and_buffers: &[(u16, usize, Vec<u8>)],
) {
    for (value, serialized_len, buffer) in values_serialized_lengths_and_buffers.iter() {
        let mut offset = 0;
        let read_value = optimized_read_compressed_u16(black_box(buffer), &mut offset).unwrap();
        assert_eq!(read_value, *value, "Value mismatch for: {}", value);
        assert_eq!(offset, *serialized_len, "Offset mismatch for: {}", value);
    }
}

criterion_group!(benches, bench_u16_parsing);
criterion_main!(benches);
