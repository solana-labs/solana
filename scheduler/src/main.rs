use log::*;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;
use sha2::{Digest, Sha256};
use rand::Rng;
use solana_metrics::datapoint_info;
use solana_measure::measure::Measure;
use solana_entry::entry::Entry;

#[derive(Default, Debug)]
struct ExecutionEnvironment {
    accounts: Vec<i8>,
    cu: usize,
}

impl ExecutionEnvironment {
    fn new(cu: usize) -> Self {
        Self {cu, ..Self::default()}
    }
}

fn schedule(entry: Entry, bank: solana_runtime::bank::Bank) {
    let tx_queue = std::collections::BTreeMap::default();

    for (ix, tx) in entry.transactions.iter().enumerate() {
        let tx = bank.verify_transaction(&tx, solana_sdk::transaction::TransactionVerificationMode::FullVerification).unwrap();
        let sig = tx.signature();
        let locks = tx.get_account_locks();
        //tx.foo();
        tx_queue.insert(ix, tx);
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
