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
    map: AddressBookMap,
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
            Entry::Vacant(entry) => todo!(),
        }

        Ok(AddressGuard { account: () })
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
    map: std::collections::BTreeMap<Fee, Task>,
}

impl TransactionQueue {
    fn add(&mut self, fee: Fee, task: Task) {
        panic!()
    }

    fn tasks(&self) -> impl std::iter::Iterator<Item = &Task> {
        self.map.values()
    }
}

fn try_lock_for_tx(
    address_book: &mut AddressBook,
    tx: &SanitizedTransaction,
) -> Result<Vec<AddressGuard>, ()> {
    let sig = tx.signature();
    let locks = tx.get_account_locks().unwrap();
    let writable_guards = locks
        .writable
        .into_iter()
        .map(|&a| address_book.try_lock_address(a))
        .collect::<Result<Vec<_>, ()>>();

    writable_guards
}

fn create_execution_environment(guards: Vec<AddressGuard>) -> ExecutionEnvironment {
    panic!()
}

fn send_to_execution_stage(ee: ExecutionEnvironment) {}

fn pop_from_queue(
    tx_queue: &mut TransactionQueue,
    address_book: &mut AddressBook,
    entry: &Entry,
    bank: &solana_runtime::bank::Bank,
) -> ExecutionEnvironment {
    for next_task in tx_queue.tasks() {
        match try_lock_for_tx(address_book, &next_task.tx) {
            Ok(lock_guards) => {
                return create_execution_environment(lock_guards);
            }
            Err(_) => {}
        }
    }

    panic!();
}

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
        // release address guards
        // clone updated Accounts into address book
        // async-ly propagate the result to rpc subsystems
    }

    fn push_to_queue(tx: VersionedTransaction, tx_queue: &mut TransactionQueue, bank: &solana_runtime::bank::Bank) {
        let ix = 23;
        let tx = bank
            .verify_transaction(
                tx,
                solana_sdk::transaction::TransactionVerificationMode::FullVerification,
            )
            .unwrap();
        //tx.foo();
        tx_queue.add(
            Fee {
                ix,
                random_sequence: 32322,
            },
            Task { tx },
        );
    }

    fn run(
        tx_queue: &mut TransactionQueue,
        address_book: &mut AddressBook,
        entry: Entry,
        bank: solana_runtime::bank::Bank,
        from_previous_stage: crossbeam_channel::Receiver<VersionedTransaction>,
        to_execution_stage: crossbeam_channel::Sender<ExecutionEnvironment>,
        from_execution_stage: crossbeam_channel::Receiver<ExecutionEnvironment>,
        to_next_stage: crossbeam_channel::Sender<ExecutionEnvironment>, // assume unbounded
    ) {
        use crossbeam_channel::select;
        let exit = true;
        while exit {
            select! {
                recv(from_previous_stage) -> tx => {
                    Self::push_to_queue(tx.unwrap(), tx_queue, &bank)
                }
                send(to_execution_stage, pop_from_queue(tx_queue, address_book, &entry, &bank)) -> res => {
                    res.unwrap();
                }
                recv(from_execution_stage) -> msg => {
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
