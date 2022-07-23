use log::*;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;

#[derive(Default, Debug)]
struct ExecutionEnvironment {
    accounts: Vec<i8>,
}

fn main() {
    solana_logger::setup();
    error!("hello");
    let (s, r) = bounded(1000);

    let mut joins = (0..5).map(|thx| {
        let s = s.clone();
        std::thread::spawn(move || {
            let mut i = 0;
            for _ in 0..3 {
                let ss = (thx, i, std::time::Instant::now(), ExecutionEnvironment::default());
                s.send(ss).unwrap();
                i += 1;
            }
        })
    }).collect::<Vec<_>>();

    joins.push(std::thread::spawn(move || {
        let mut count = 0;
        let start = std::time::Instant::now();
        let mut rrr = Vec::new_with_capacity(10);
        for _ in 0..10 {
            rrr.push(r.recv().unwrap());
        }

        for rr in rrr {
            count += 1;
            error!("recv-ed: {:?} latency: {}", rr, rr.2.elapsed().as_nanos());
            if count % 100_000 == 0 {
                error!("recv-ed: {}", count / start.elapsed().as_secs().max(1));
                break
            }
        }
    }));
    joins.into_iter().for_each(|j| j.join().unwrap());
}
