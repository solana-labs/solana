use {
    criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput},
    rand::seq::SliceRandom,
    solana_accounts_db::{
        accounts_db::AccountsDb,
        accounts_hash::{AccountHash, AccountsHasher},
    },
    solana_sdk::{account::AccountSharedData, hash::Hash, pubkey::Pubkey},
};

const KB: usize = 1024;
const MB: usize = KB * KB;

const DATA_SIZES: [usize; 6] = [
    0,       // the smallest account
    165,     // the size of an spl token account
    200,     // the size of a stake account
    KB,      // a medium sized account
    MB,      // a large sized account
    10 * MB, // the largest account
];

/// The number of bytes of *non account data* that are also hashed as
/// part of computing an account's hash.
///
/// Ensure this constant stays in sync with the value of `META_SIZE` in
/// AccountsDb::hash_account_data().
const META_SIZE: usize = 81;

fn bench_hash_account(c: &mut Criterion) {
    let lamports = 123_456_789;
    let owner = Pubkey::default();
    let address = Pubkey::default();

    let mut group = c.benchmark_group("hash_account");
    for data_size in DATA_SIZES {
        let num_bytes = META_SIZE.checked_add(data_size).unwrap();
        group.throughput(Throughput::Bytes(num_bytes as u64));
        let account = AccountSharedData::new(lamports, data_size, &owner);
        group.bench_function(BenchmarkId::new("data_size", data_size), |b| {
            b.iter(|| AccountsDb::hash_account(&account, &address));
        });
    }
}

fn bench_accounts_delta_hash(c: &mut Criterion) {
    const ACCOUNTS_COUNTS: [usize; 4] = [
        1,      // the smallest count; will bench overhead
        100,    // number of accounts written per slot on mnb (with *no* rent rewrites)
        1_000,  // number of accounts written slot on mnb (with rent rewrites)
        10_000, // reasonable largest number of accounts written per slot
    ];

    fn create_account_hashes(accounts_count: usize) -> Vec<(Pubkey, AccountHash)> {
        let mut account_hashes: Vec<_> = std::iter::repeat_with(|| {
            let address = Pubkey::new_unique();
            let hash = AccountHash(Hash::new_unique());
            (address, hash)
        })
        .take(accounts_count)
        .collect();

        // since the accounts delta hash needs to sort the accounts first, ensure we're not
        // creating a pre-sorted vec.
        let mut rng = rand::thread_rng();
        account_hashes.shuffle(&mut rng);
        account_hashes
    }

    let mut group = c.benchmark_group("accounts_delta_hash");
    for accounts_count in ACCOUNTS_COUNTS {
        group.throughput(Throughput::Elements(accounts_count as u64));
        let account_hashes = create_account_hashes(accounts_count);
        group.bench_function(BenchmarkId::new("accounts_count", accounts_count), |b| {
            b.iter_batched(
                || account_hashes.clone(),
                AccountsHasher::accumulate_account_hashes,
                BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(benches, bench_hash_account, bench_accounts_delta_hash);
criterion_main!(benches);
