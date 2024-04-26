use {
    criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput},
    serde::{Deserialize, Serialize},
    solana_sdk::{account::Account, clock::Epoch, pubkey::Pubkey},
    std::mem,
};

const KB: usize = 1024;
const MB: usize = KB * KB;

const DATA_SIZES: [usize; 4] = [
    0,       // the smallest account
    200,     // the size of a stake account
    MB,      // the max size of bincode's internal buffer
    10 * MB, // the largest account
];

/// Benchmark how long it takes to serialize an account
fn bench_account_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_account_serialize");
    for data_size in DATA_SIZES {
        let account = Account::new(0, data_size, &Pubkey::default());
        let num_bytes = bincode::serialized_size(&account).unwrap();
        group.throughput(Throughput::Bytes(num_bytes));

        // serialize the account, treating the data as a sequence
        let account_with_data_as_seq = AccountWithDataAsSeq {
            data: vec![0; data_size],
            ..Default::default()
        };
        group.bench_function(BenchmarkId::new("data_as_seq", data_size), |b| {
            b.iter_batched(
                || Vec::with_capacity(num_bytes as usize),
                |buffer| bincode::serialize_into(buffer, &account_with_data_as_seq).unwrap(),
                BatchSize::PerIteration,
            );
        });

        // serialize the account, treating the data as bytes
        let account_with_data_as_bytes = AccountWithDataAsBytes {
            data: vec![0; data_size],
            ..Default::default()
        };
        group.bench_function(BenchmarkId::new("data_as_bytes", data_size), |b| {
            b.iter_batched(
                || Vec::with_capacity(num_bytes as usize),
                |buffer| bincode::serialize_into(buffer, &account_with_data_as_bytes).unwrap(),
                BatchSize::PerIteration,
            );
        });
    }
}

/// Benchmark how long it takes to deserialize an account
fn bench_account_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_account_deserialize");
    for data_size in DATA_SIZES {
        let account = Account::new(0, data_size, &Pubkey::default());
        let serialized_account = bincode::serialize(&account).unwrap();
        let num_bytes = serialized_account.len() as u64;
        group.throughput(Throughput::Bytes(num_bytes));

        // deserialize the account, treating the data as a sequence
        let serialized_account = bincode::serialize(&AccountWithDataAsSeq {
            data: vec![0; data_size],
            ..Default::default()
        })
        .unwrap();
        group.bench_function(BenchmarkId::new("data_as_seq", data_size), |b| {
            b.iter_batched(
                || (),
                |()| {
                    bincode::deserialize::<AccountWithDataAsSeq>(serialized_account.as_slice())
                        .unwrap()
                },
                BatchSize::PerIteration,
            );
        });

        // deserialize the account, treating the data as bytes
        let serialized_account = bincode::serialize(&AccountWithDataAsBytes {
            data: vec![0; data_size],
            ..Default::default()
        })
        .unwrap();
        group.bench_function(BenchmarkId::new("data_as_bytes", data_size), |b| {
            b.iter_batched(
                || (),
                |()| {
                    bincode::deserialize::<AccountWithDataAsBytes>(serialized_account.as_slice())
                        .unwrap()
                },
                BatchSize::PerIteration,
            );
        });
    }
}

/// An account, with normal serde behavior.
/// The account data will be serialized as a sequence.
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
struct AccountWithDataAsSeq {
    lamports: u64,
    data: Vec<u8>,
    owner: Pubkey,
    executable: bool,
    rent_epoch: Epoch,
}

// Ensure that our new account type stays in-sync with the real Account type.
const _: () = assert!(mem::size_of::<AccountWithDataAsSeq>() == mem::size_of::<Account>());

/// An account, with modified serde behavior.
/// The account data will be serialized as bytes.
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
struct AccountWithDataAsBytes {
    lamports: u64,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
    owner: Pubkey,
    executable: bool,
    rent_epoch: Epoch,
}

// Ensure that our new account type stays in-sync with the real Account type.
const _: () = assert!(mem::size_of::<AccountWithDataAsBytes>() == mem::size_of::<Account>());

criterion_group!(benches, bench_account_serialize, bench_account_deserialize);
criterion_main!(benches);
