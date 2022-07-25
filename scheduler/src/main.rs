#![feature(map_first_last)]

use {
    atomic_enum::atomic_enum,
    crossbeam_channel::{bounded, unbounded},
    log::*,
    rand::Rng,
    sha2::{Digest, Sha256},
    solana_entry::entry::Entry,
    solana_measure::measure::Measure,
    solana_metrics::datapoint_info,
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
};
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::transaction::TransactionAccountLocks;
use solana_sdk::hash::Hash;

#[derive(Default, Debug)]
struct ExecutionEnvironment {
    accounts: Vec<i8>,
    cu: usize,
    //tx: Tx,
}

impl ExecutionEnvironment {
    fn new(cu: usize) -> Self {
        Self {
            cu,
            ..Self::default()
        }
    }

    //fn abort() {
    //  pass AtomicBool into InvokeContext??
    //}
}

struct LockAttempt {
    address: Pubkey,
    is_success: bool,
}

impl LockAttempt {
    fn is_success(&self) -> bool {
        self.is_success
    }

    fn success(address: Pubkey) -> Self {
        Self { address, is_success: true }
    }

    fn failure(address: Pubkey) -> Self {
        Self { address, is_success: false }
    }
}

#[derive(PartialEq)]
enum Usage {
    Unused,
    Readonly,
    Writable,
}

struct Page {
    current_usage: Usage,
}

type AddressBookMap = std::collections::BTreeMap<Pubkey, Page>;

// needs ttl mechanism and prune
struct AddressBook {
    map: AddressBookMap,
}

impl AddressBook {
    fn try_lock_address(&mut self, address: Pubkey, requested_usage: Usage) -> LockAttempt {
        use std::collections::btree_map::Entry;

        match self.map.entry(address) {
            // unconditional success if it's initial access
            Entry::Vacant(entry) => {
                entry.insert(Page {
                    current_usage: requested_usage,
                });
                LockAttempt::success(address)
            }
            Entry::Occupied(mut entry) => {
                let mut page = entry.get_mut();

                match &page.current_usage {
                    Usage::Unused => {
                        page.current_usage = requested_usage;
                        LockAttempt::success(address) 
                    }
                    Usage::Readonly => {
                        match &requested_usage {
                            Usage::Readonly => {
                                LockAttempt::success(address)
                            },
                            Usage::Writable => {
                                // add to contended queue?
                                LockAttempt::failure(address)
                            }
                            Usage::Unused => {
                                page.current_usage = requested_usage;
                                LockAttempt::success(address)
                            }
                        }
                    }
                    Usage::Writable => {
                        match &requested_usage {
                            Usage::Readonly | Usage::Writable => panic!(),
                            Usage::Unused => unreachable!(),
                        }
                    }
                }
            }
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Weight { // naming: Sequence Ordering?
    ix: usize, // index in ledger entry?
    randomness: usize, // tie breaker? random noise?
    // fee?
}

struct Bundle {
}

struct Task {
    tx: SanitizedTransaction, // actually should be Bundle
}

// RunnableQueue, ContendedQueue?
struct TransactionQueue {
    map: std::collections::BTreeMap<Weight, Task>,
}

impl TransactionQueue {
    fn add(&mut self, weight: Weight, task: Task) {
        self.map.insert(weight, task).unwrap();
    }

    fn tasks(&self) -> impl std::iter::Iterator<Item = &Task> {
        self.map.values()
    }
}

fn try_lock_for_tx<'a>(
    address_book: &mut AddressBook,
    message_hash: &'a Hash,
    locks: &'a TransactionAccountLocks,
) -> Vec<LockAttempt> {
    let writable_guards = locks
        .writable
        .iter()
        .cloned()
        .map(|&a| address_book.try_lock_address(a, Usage::Writable))
        .collect::<Vec<_>>();

    writable_guards
}

fn create_execution_environment(guards: Vec<LockAttempt>) -> ExecutionEnvironment {
    panic!()
}

fn send_to_execution_stage(ee: ExecutionEnvironment) {}

fn main() {
    solana_logger::setup();
    error!("hello");
    let thread_count = 10;
    let (s, r) = bounded::<(
        (usize, usize, (std::time::Instant, ExecutionEnvironment)),
        Vec<u8>,
    )>(thread_count * 10);
    let (s2, r2) = bounded(thread_count * 2);

    /*
    let p = std::thread::Builder::new().name("producer".to_string()).spawn(move || {
        let mut rng = rand::thread_rng();
        loop {
            s2.send((std::time::Instant::now(), ExecutionEnvironment::new(rng.gen_range(0, 1000)))).unwrap();
        }
    }).unwrap();
    */
    let pc = std::thread::Builder::new().name("prosumer".to_string()).spawn(move || {
        use crossbeam_channel::select;

        let mut rng = rand::thread_rng();
        let mut count = 0;
        let start = std::time::Instant::now();
        //let mut rrr = Vec::with_capacity(10);
        //for _ in 0..100 {
        let mut elapsed = 0;

        loop {
            select! {
                send(s2, (std::time::Instant::now(), ExecutionEnvironment::new(rng.gen_range(0, 1000)))) -> res => {
                    res.unwrap();
                }
                recv(r) -> msg => {
                    let rr = msg.unwrap();
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
                }
        }
    }).unwrap();

    let mut joins = (0..thread_count)
        .map(|thx| {
            let s = s.clone();
            let r2 = r2.clone();
            std::thread::Builder::new()
                .name(format!("blockstore_processor_{}", thx))
                .spawn(move || {
                    let current_thread_name = std::thread::current().name().unwrap().to_string();
                    let mut i = 0;
                    //for _ in 0..60 {//000000 {
                    loop {
                        let ss = (thx, i, r2.recv().unwrap());

                        let mut process_message_time = Measure::start("process_message_time");

                        let mut hasher = Sha256::default();
                        let cu = ss.2 .1.cu;
                        for i in 0_usize..cu {
                            //for _ in 0..10 {
                            hasher.update(i.to_le_bytes());
                            //}
                        }
                        let h = hasher.finalize();

                        process_message_time.stop();
                        let duration_with_overhead = process_message_time.as_us();

                        /*
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
                        */
                        s.send((ss, h[0..10].into_iter().copied().collect::<Vec<_>>()))
                            .unwrap();
                        i += 1;
                    }
                })
                .unwrap()
        })
        .collect::<Vec<_>>();

    //joins.push(p);

    joins.push(pc);
    joins.into_iter().for_each(|j| j.join().unwrap());
}

struct ScheduleStage {
}

impl ScheduleStage {
    fn commit(ee: &mut ExecutionEnvironment) {
        // par()-ly release address guards
        // par()-ly clone updated Accounts into address book
        // async-ly propagate the result to rpc subsystems
    }

    fn push_to_queue((weight, tx): (Weight, SanitizedTransaction), tx_queue: &mut TransactionQueue, bank: &solana_runtime::bank::Bank) {
        //let ix = 23;
        //let tx = bank
        //    .verify_transaction(
        //        tx,
        //        solana_sdk::transaction::TransactionVerificationMode::FullVerification,
        //    )
        //    .unwrap();
        //tx.foo();
        tx_queue.add(
            weight, 
            Task { tx },
        );
    }

    fn pop_from_queue(
        tx_queue: &mut TransactionQueue,
        address_book: &mut AddressBook,
        entry: &Entry,
        bank: &solana_runtime::bank::Bank,
    ) -> ExecutionEnvironment {
        for next_task in tx_queue.tasks() {
            let message_hash = next_task.tx.message_hash();
            let locks = next_task.tx.get_account_locks().unwrap();
            let lock_guards = try_lock_for_tx(address_book, &message_hash, &locks);
            if lock_guards.iter().all(|g| g.is_success()) {
                return create_execution_environment(lock_guards);
            }
        }

        panic!();
    }

    fn run(
        tx_queue: &mut TransactionQueue,
        address_book: &mut AddressBook,
        entry: Entry,
        bank: solana_runtime::bank::Bank,
        from_previous_stage: crossbeam_channel::Receiver<(Weight, SanitizedTransaction)>,
        to_execute_stage: crossbeam_channel::Sender<ExecutionEnvironment>,
        from_execute_stage: crossbeam_channel::Receiver<ExecutionEnvironment>,
        to_next_stage: crossbeam_channel::Sender<ExecutionEnvironment>, // assume unbounded
    ) {
        use crossbeam_channel::select;
        let exit = true;
        while exit {
            select! {
                recv(from_previous_stage) -> weighted_tx => {
                    Self::push_to_queue(weighted_tx.unwrap(), tx_queue, &bank)
                }
                send(to_execute_stage, Self::pop_from_queue(tx_queue, address_book, &entry, &bank)) -> res => {
                    res.unwrap();
                }
                recv(from_execute_stage) -> msg => {
                    let mut msg = msg.unwrap();
                    Self::commit(&mut msg);
                    to_next_stage.send(msg).unwrap()
                }
            }
        }
    }
}

struct ExecuteStage {
    //bank: Bank,
}

impl ExecuteStage {
}
