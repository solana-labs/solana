use log::*;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;
use sha2::{Digest, Sha256};
use rand::Rng;

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

fn main() {
    solana_logger::setup();
    error!("hello");
    let thread_count = 10;
    let (s, r) = bounded(thread_count * 10);
    let (s2, r2) = bounded(thread_count * 2);

    let p = std::thread::spawn(move || {
        let mut rng = rand::thread_rng();
        loop {
            s2.send((std::time::Instant::now(), ExecutionEnvironment::new(rng.gen_range(0, 100)))).unwrap();
        }
    });

    let mut joins = (0..thread_count).map(|thx| {
        let s = s.clone();
        let r2 = r2.clone();
        std::thread::spawn(move || {
            let mut i = 0;
            for _ in 0..60 {//000000 {
            //loop {
                let ss = (thx, i, r2.recv().unwrap());
                let mut hasher = Sha256::default();
                for i in 0_usize..ss.2.1.cu {
                    //for _ in 0..10 {
                        hasher.update(i.to_le_bytes());
                    //}
                }
                let h = hasher.finalize();
                s.send((ss, h[0..10].into_iter().copied().collect::<Vec<_>>())).unwrap();
                datapoint_info!(
                    "individual_tx_stats",
                    ("slot", slot, i64),
                    ("thread", current_thread_name, String),
                    ("signature", signature, String),
                    ("account_locks_in_json", account_locks_in_json, String),
                    ("status", process_result_in_debug, String),
                    ("duration", duration_with_overhead, i64),
                    ("compute_units", executed_units, i64),
                );
                i += 1;
            }
        })
    }).collect::<Vec<_>>();

    joins.push(p);

    joins.push(std::thread::spawn(move || {
        let mut count = 0;
        let start = std::time::Instant::now();
        let mut rrr = Vec::with_capacity(10);
        for _ in 0..100 {
        //loop {
            let rr = r.recv().unwrap();
            rrr.push((rr.0.2.0.elapsed(), rr));
        }

        for rr in rrr {
            count += 1;
            error!("recv-ed: {:?}", &rr);
            if count % 100_000 == 0 {
                error!("recv-ed: {}", count / start.elapsed().as_secs().max(1));
                //break
            }
        }
    }));
    joins.into_iter().for_each(|j| j.join().unwrap());
}
