#![allow(clippy::arithmetic_side_effects)]
use {
    criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput},
    solana_accounts_db::{
        append_vec::{self, AppendVec},
        tiered_storage::hot::HotStorageWriter,
    },
    solana_sdk::{
        account::AccountSharedData, clock::Slot, pubkey::Pubkey,
        rent_collector::RENT_EXEMPT_RENT_EPOCH,
    },
};

const ACCOUNTS_COUNTS: [usize; 4] = [
    1,      // the smallest count; will bench overhead
    100,    // number of accounts written per slot on mnb (with *no* rent rewrites)
    1_000,  // number of accounts written slot on mnb (with rent rewrites)
    10_000, // reasonable largest number of accounts written per slot
];

fn bench_write_accounts_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_accounts_file");

    // most accounts on mnb are 165-200 bytes, so use that here too
    let space = 200;
    let lamports = 2_282_880; // the rent-exempt amount for 200 bytes of data
    let temp_dir = tempfile::tempdir().unwrap();

    for accounts_count in ACCOUNTS_COUNTS {
        group.throughput(Throughput::Elements(accounts_count as u64));

        let accounts: Vec<_> = std::iter::repeat_with(|| {
            (
                Pubkey::new_unique(),
                AccountSharedData::new_rent_epoch(
                    lamports,
                    space,
                    &Pubkey::new_unique(),
                    RENT_EXEMPT_RENT_EPOCH,
                ),
            )
        })
        .take(accounts_count)
        .collect();
        let accounts_refs: Vec<_> = accounts
            .iter()
            .map(|(pubkey, account)| (pubkey, account))
            .collect();
        let storable_accounts = (Slot::MAX, accounts_refs.as_slice());

        group.bench_function(BenchmarkId::new("append_vec", accounts_count), |b| {
            b.iter_batched_ref(
                || {
                    let path = temp_dir.path().join(format!("append_vec_{accounts_count}"));
                    let file_size = accounts.len() * (space + append_vec::STORE_META_OVERHEAD);
                    AppendVec::new(path, true, file_size)
                },
                |append_vec| {
                    let res = append_vec.append_accounts(&storable_accounts, 0).unwrap();
                    let accounts_written_count = res.offsets.len();
                    assert_eq!(accounts_written_count, accounts_count);
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_function(BenchmarkId::new("hot_storage", accounts_count), |b| {
            b.iter_batched_ref(
                || {
                    let path = temp_dir
                        .path()
                        .join(format!("hot_storage_{accounts_count}"));
                    _ = std::fs::remove_file(&path);
                    HotStorageWriter::new(path).unwrap()
                },
                |hot_storage| {
                    let res = hot_storage.write_accounts(&storable_accounts, 0).unwrap();
                    let accounts_written_count = res.offsets.len();
                    assert_eq!(accounts_written_count, accounts_count);
                    // Purposely do not call hot_storage.flush() here, since it will impact the
                    // bench.  Flushing will be handled by Drop, which is *not* timed (and that's
                    // what we want).
                },
                BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(benches, bench_write_accounts_file);
criterion_main!(benches);
