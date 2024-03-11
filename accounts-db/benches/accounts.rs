#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

extern crate test;

use {
    dashmap::DashMap,
    rand::Rng,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    solana_accounts_db::{
        accounts::{AccountAddressFilter, Accounts},
        accounts_db::{
            test_utils::create_test_accounts, AccountShrinkThreshold, AccountsDb,
            VerifyAccountsHashAndLamportsConfig, ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS,
        },
        accounts_index::{AccountSecondaryIndexes, ScanConfig},
        ancestors::Ancestors,
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        genesis_config::ClusterType,
        hash::Hash,
        pubkey::Pubkey,
        rent_collector::RentCollector,
        sysvar::epoch_schedule::EpochSchedule,
    },
    std::{
        collections::{HashMap, HashSet},
        path::PathBuf,
        sync::{Arc, RwLock},
        thread::Builder,
    },
    test::Bencher,
};

fn new_accounts_db(account_paths: Vec<PathBuf>) -> AccountsDb {
    AccountsDb::new_with_config(
        account_paths,
        &ClusterType::Development,
        AccountSecondaryIndexes::default(),
        AccountShrinkThreshold::default(),
        Some(ACCOUNTS_DB_CONFIG_FOR_BENCHMARKS),
        None,
        Arc::default(),
    )
}

#[bench]
fn bench_accounts_hash_bank_hash(bencher: &mut Bencher) {
    let accounts_db = new_accounts_db(vec![PathBuf::from("bench_accounts_hash_internal")]);
    let accounts = Accounts::new(Arc::new(accounts_db));
    let mut pubkeys: Vec<Pubkey> = vec![];
    let num_accounts = 60_000;
    let slot = 0;
    create_test_accounts(&accounts, &mut pubkeys, num_accounts, slot);
    let ancestors = Ancestors::from(vec![0]);
    let (_, total_lamports) = accounts
        .accounts_db
        .update_accounts_hash_for_tests(0, &ancestors, false, false);
    accounts.add_root(slot);
    accounts.accounts_db.flush_accounts_cache(true, Some(slot));
    bencher.iter(|| {
        assert!(accounts.verify_accounts_hash_and_lamports(
            0,
            total_lamports,
            None,
            VerifyAccountsHashAndLamportsConfig {
                ancestors: &ancestors,
                test_hash_calculation: false,
                epoch_schedule: &EpochSchedule::default(),
                rent_collector: &RentCollector::default(),
                ignore_mismatch: false,
                store_detailed_debug_info: false,
                use_bg_thread_pool: false,
            }
        ))
    });
}

#[bench]
fn bench_update_accounts_hash(bencher: &mut Bencher) {
    solana_logger::setup();
    let accounts_db = new_accounts_db(vec![PathBuf::from("update_accounts_hash")]);
    let accounts = Accounts::new(Arc::new(accounts_db));
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 50_000, 0);
    let ancestors = Ancestors::from(vec![0]);
    bencher.iter(|| {
        accounts
            .accounts_db
            .update_accounts_hash_for_tests(0, &ancestors, false, false);
    });
}

#[bench]
fn bench_accounts_delta_hash(bencher: &mut Bencher) {
    solana_logger::setup();
    let accounts_db = new_accounts_db(vec![PathBuf::from("accounts_delta_hash")]);
    let accounts = Accounts::new(Arc::new(accounts_db));
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 100_000, 0);
    bencher.iter(|| {
        accounts.accounts_db.calculate_accounts_delta_hash(0);
    });
}

#[bench]
fn bench_delete_dependencies(bencher: &mut Bencher) {
    solana_logger::setup();
    let accounts_db = new_accounts_db(vec![PathBuf::from("accounts_delete_deps")]);
    let accounts = Accounts::new(Arc::new(accounts_db));
    let mut old_pubkey = Pubkey::default();
    let zero_account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
    for i in 0..1000 {
        let pubkey = solana_sdk::pubkey::new_rand();
        let account = AccountSharedData::new(i + 1, 0, AccountSharedData::default().owner());
        accounts.store_slow_uncached(i, &pubkey, &account);
        accounts.store_slow_uncached(i, &old_pubkey, &zero_account);
        old_pubkey = pubkey;
        accounts.add_root(i);
    }
    bencher.iter(|| {
        accounts.accounts_db.clean_accounts_for_tests();
    });
}

fn store_accounts_with_possible_contention<F: 'static>(
    bench_name: &str,
    bencher: &mut Bencher,
    reader_f: F,
) where
    F: Fn(&Accounts, &[Pubkey]) + Send + Copy,
{
    let num_readers = 5;
    let accounts_db = new_accounts_db(vec![PathBuf::from(
        std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string()),
    )
    .join(bench_name)]);
    let accounts = Arc::new(Accounts::new(Arc::new(accounts_db)));
    let num_keys = 1000;
    let slot = 0;

    let pubkeys: Vec<_> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
        .take(num_keys)
        .collect();
    let accounts_data: Vec<_> = std::iter::repeat(Account {
        lamports: 1,
        ..Default::default()
    })
    .take(num_keys)
    .collect();
    let storable_accounts: Vec<_> = pubkeys.iter().zip(accounts_data.iter()).collect();
    accounts.store_accounts_cached((slot, storable_accounts.as_slice()));
    accounts.add_root(slot);
    accounts
        .accounts_db
        .flush_accounts_cache_slot_for_tests(slot);

    let pubkeys = Arc::new(pubkeys);
    for i in 0..num_readers {
        let accounts = accounts.clone();
        let pubkeys = pubkeys.clone();
        Builder::new()
            .name(format!("reader{i:02}"))
            .spawn(move || {
                reader_f(&accounts, &pubkeys);
            })
            .unwrap();
    }

    let num_new_keys = 1000;
    bencher.iter(|| {
        let new_pubkeys: Vec<_> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .take(num_new_keys)
            .collect();
        let new_storable_accounts: Vec<_> = new_pubkeys.iter().zip(accounts_data.iter()).collect();
        // Write to a different slot than the one being read from. Because
        // there's a new account pubkey being written to every time, will
        // compete for the accounts index lock on every store
        accounts.store_accounts_cached((slot + 1, new_storable_accounts.as_slice()));
    });
}

#[bench]
fn bench_concurrent_read_write(bencher: &mut Bencher) {
    store_accounts_with_possible_contention(
        "concurrent_read_write",
        bencher,
        |accounts, pubkeys| {
            let mut rng = rand::thread_rng();
            loop {
                let i = rng.gen_range(0..pubkeys.len());
                test::black_box(
                    accounts
                        .load_without_fixed_root(&Ancestors::default(), &pubkeys[i])
                        .unwrap(),
                );
            }
        },
    )
}

#[bench]
fn bench_concurrent_scan_write(bencher: &mut Bencher) {
    store_accounts_with_possible_contention("concurrent_scan_write", bencher, |accounts, _| loop {
        test::black_box(
            accounts
                .load_by_program(
                    &Ancestors::default(),
                    0,
                    AccountSharedData::default().owner(),
                    &ScanConfig::default(),
                )
                .unwrap(),
        );
    })
}

#[bench]
#[ignore]
fn bench_dashmap_single_reader_with_n_writers(bencher: &mut Bencher) {
    let num_readers = 5;
    let num_keys = 10000;
    let map = Arc::new(DashMap::new());
    for i in 0..num_keys {
        map.insert(i, i);
    }
    for _ in 0..num_readers {
        let map = map.clone();
        Builder::new()
            .name("readers".to_string())
            .spawn(move || loop {
                test::black_box(map.entry(5).or_insert(2));
            })
            .unwrap();
    }
    bencher.iter(|| {
        for _ in 0..num_keys {
            test::black_box(map.get(&5).unwrap().value());
        }
    })
}

#[bench]
#[ignore]
fn bench_rwlock_hashmap_single_reader_with_n_writers(bencher: &mut Bencher) {
    let num_readers = 5;
    let num_keys = 10000;
    let map = Arc::new(RwLock::new(HashMap::new()));
    for i in 0..num_keys {
        map.write().unwrap().insert(i, i);
    }
    for _ in 0..num_readers {
        let map = map.clone();
        Builder::new()
            .name("readers".to_string())
            .spawn(move || loop {
                test::black_box(map.write().unwrap().get(&5));
            })
            .unwrap();
    }
    bencher.iter(|| {
        for _ in 0..num_keys {
            test::black_box(map.read().unwrap().get(&5));
        }
    })
}

fn setup_bench_dashmap_iter() -> (Arc<Accounts>, DashMap<Pubkey, (AccountSharedData, Hash)>) {
    let accounts_db = new_accounts_db(vec![PathBuf::from(
        std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string()),
    )
    .join("bench_dashmap_par_iter")]);
    let accounts = Arc::new(Accounts::new(Arc::new(accounts_db)));

    let dashmap = DashMap::new();
    let num_keys = std::env::var("NUM_BENCH_KEYS")
        .map(|num_keys| num_keys.parse::<usize>().unwrap())
        .unwrap_or_else(|_| 10000);
    for _ in 0..num_keys {
        dashmap.insert(
            Pubkey::new_unique(),
            (
                AccountSharedData::new(1, 0, AccountSharedData::default().owner()),
                Hash::new_unique(),
            ),
        );
    }

    (accounts, dashmap)
}

#[bench]
fn bench_dashmap_par_iter(bencher: &mut Bencher) {
    let (accounts, dashmap) = setup_bench_dashmap_iter();

    bencher.iter(|| {
        test::black_box(accounts.accounts_db.thread_pool.install(|| {
            dashmap
                .par_iter()
                .map(|cached_account| (*cached_account.key(), cached_account.value().1))
                .collect::<Vec<(Pubkey, Hash)>>()
        }));
    });
}

#[bench]
fn bench_dashmap_iter(bencher: &mut Bencher) {
    let (_accounts, dashmap) = setup_bench_dashmap_iter();

    bencher.iter(|| {
        test::black_box(
            dashmap
                .iter()
                .map(|cached_account| (*cached_account.key(), cached_account.value().1))
                .collect::<Vec<(Pubkey, Hash)>>(),
        );
    });
}

#[bench]
fn bench_load_largest_accounts(b: &mut Bencher) {
    let accounts_db = new_accounts_db(Vec::new());
    let accounts = Accounts::new(Arc::new(accounts_db));
    let mut rng = rand::thread_rng();
    for _ in 0..10_000 {
        let lamports = rng.gen();
        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::new(lamports, 0, &Pubkey::default());
        accounts.store_slow_uncached(0, &pubkey, &account);
    }
    let ancestors = Ancestors::from(vec![0]);
    let bank_id = 0;
    b.iter(|| {
        accounts.load_largest_accounts(
            &ancestors,
            bank_id,
            20,
            &HashSet::new(),
            AccountAddressFilter::Exclude,
        )
    });
}
