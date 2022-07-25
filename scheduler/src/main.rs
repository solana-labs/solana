#![feature(map_first_last)]

use log::*;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;
use sha2::{Digest, Sha256};
use rand::Rng;
use solana_metrics::datapoint_info;
use solana_measure::measure::Measure;
use solana_entry::entry::Entry;
use solana_sdk::transaction::SanitizedTransaction;
use solana_sdk::pubkey::Pubkey;
use atomic_enum::atomic_enum;

#[derive(Default, Debug)]
struct ExecutionEnvironment {
    accounts: Vec<i8>,
    cu: usize,
    //tx: Tx,
}

impl ExecutionEnvironment {
    fn new(cu: usize) -> Self {
        Self {cu, ..Self::default()}
    }

    //fn abort() {
    //  pass AtomicBool into InvokeContext??
    //}

    fn commit() {
        // release address guards
        // clone updated Accounts into address book
        // async-ly propagate the result to rpc subsystems
    }
}


struct AddressGuard {
    account: (),
} 

#[atomic_enum]
#[derive(PartialEq)]
enum Usage {
    Unused,
    Readonly,
    Writable,
}

struct Page {
    usage: AtomicUsage,
}

type AddressBookMap = std::collections::BTreeMap<Pubkey, Page>;

struct AddressBook {
    map: AddressBookMap ,
}

impl AddressBook {
    fn try_lock_address(&mut self, address: Pubkey) -> Result<AddressGuard, ()> {
        use std::collections::btree_map::Entry;

        match self.map.entry(address) {
            Entry::Occupied(entry) => {
                match &entry.get().usage.load(std::sync::atomic::Ordering::Relaxed) {
                    Usage::Unused => todo!(),
                    Usage::Readonly => todo!(),
                    Usage::Writable => return Err(()),
                }
            }
            Entry::Vacant(entry) => todo!()
        }

        Ok(AddressGuard{account: ()})
    }
}

struct Fee {
    ix: usize,
    random_sequence: usize, // tie breaker? random noise?
}

struct Task {
    tx: SanitizedTransaction,
}

// RunnableQueue, ContendedQueue?
struct TransactionQueue {
    map: std::collections::BTreeMap<Fee, Task>
}

impl TransactionQueue {
    fn add(&mut self, fee: Fee, task: Task) {
        panic!()
    }

    fn tasks(&self) -> impl std::iter::Iterator<Item = &Task> {
        self.map.values()
    }
}

fn try_lock_for_tx(address_book: &mut AddressBook, tx: &SanitizedTransaction) -> Result<Vec<AddressGuard>, ()> {
    let sig = tx.signature();
    let locks = tx.get_account_locks().unwrap();
    let writable_guards = locks.writable.into_iter().map(|&a|
        address_book.try_lock_address(a)
    ).collect::<Result<Vec<_>, ()>>();

    writable_guards
}

fn create_execution_environment(guards: Vec<AddressGuard>) -> ExecutionEnvironment {
    panic!()
}

fn send_to_execution_lane(ee: ExecutionEnvironment) {
}

fn schedule(tx_queue: &mut TransactionQueue, address_book: &mut AddressBook, entry: Entry, bank: solana_runtime::bank::Bank) {
    for (ix, tx) in entry.transactions.into_iter().enumerate() {
        let tx = bank.verify_transaction(tx, solana_sdk::transaction::TransactionVerificationMode::FullVerification).unwrap();
        //tx.foo();
        tx_queue.add(Fee {ix, random_sequence: 32322}, Task {tx});
    }
    for next_task in tx_queue.tasks() {
        if let Ok(lock_guards) = try_lock_for_tx(address_book, &next_task.tx) {
            let ee = create_execution_environment(lock_guards);
            send_to_execution_lane(ee);
        }
    }
}

fn main() {
    solana_logger::setup();
    error!("hello");
    let thread_count = 10;
    let (s, r) = bounded(thread_count * 10);
    let (s2, r2) = bounded(thread_count * 2);

    let p = std::thread::Builder::new().name("producer".to_string()).spawn(move || {
        let mut rng = rand::thread_rng();
        loop {
            s2.send((std::time::Instant::now(), ExecutionEnvironment::new(rng.gen_range(0, 1000)))).unwrap();
        }
    }).unwrap();

    let mut joins = (0..thread_count).map(|thx| {
        let s = s.clone();
        let r2 = r2.clone();
        std::thread::Builder::new().name(format!("blockstore_processor_{}", thx)).spawn(move || {
            let current_thread_name = std::thread::current().name().unwrap().to_string();
            let mut i = 0;
            //for _ in 0..60 {//000000 {
            loop {
                let ss = (thx, i, r2.recv().unwrap());

                let mut process_message_time = Measure::start("process_message_time");

                let mut hasher = Sha256::default();
                let cu = ss.2.1.cu;
                for i in 0_usize..cu {
                    //for _ in 0..10 {
                        hasher.update(i.to_le_bytes());
                    //}
                }
                let h = hasher.finalize();

                process_message_time.stop();
                let duration_with_overhead = process_message_time.as_us();

                datapoint_info!(
                    "individual_tx_stats",
                    ("slot", 33333, i64),
                    ("thread", current_thread_name, String),
                    ("signature", "ffffff", String),
                    ("account_locks_in_json", "{}", String),
                    ("status", "Ok", String),
                    ("duration", duration_with_overhead, i64),
                    ("compute_units", cu, i64),
                );
                s.send((ss, h[0..10].into_iter().copied().collect::<Vec<_>>())).unwrap();
                i += 1;
            }
        }).unwrap()
    }).collect::<Vec<_>>();

    joins.push(p);

    joins.push(std::thread::Builder::new().name("consumer".to_string()).spawn(move || {
        let mut count = 0;
        let start = std::time::Instant::now();
        //let mut rrr = Vec::with_capacity(10);
        //for _ in 0..100 {
        let mut elapsed = 0;

        loop {
            let rr = r.recv().unwrap();
            elapsed += rr.0.2.0.elapsed().as_nanos();
        //    rrr.push((rr.0.2.0.elapsed(), rr));
        //}

        //for rr in rrr {
            count += 1;
            //error!("recv-ed: {:?}", &rr);
            if count % 100_000 == 0 {
                error!("recv-ed: {}", count / start.elapsed().as_secs().max(1));
                //break
            }
        }
    }).unwrap());
    joins.into_iter().for_each(|j| j.join().unwrap());
}
