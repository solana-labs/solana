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
    let (s, r) = unbounded();

    let mut joins = (0..10).map(|thx| {
        let s = s.clone();
        std::thread::spawn(move || {
            let mut i = 0;
            for _ in 0..100_000 {
                let ss = (thx, i, ExecutionEnvironment::default());
                error!("send-ed: {:?}", ss);
                s.send(ss).unwrap();
                i += 1;
            }
        })
    }).collect::<Vec<_>>();

    joins.push(std::thread::spawn(move || {
        let mut count = 0;
        let start = std::time::Instant::now();
        loop {
            let rr = r.recv().unwrap();
            count += 1;
            error!("recv-ed: {:?}", rr);
            if count % 100_000 == 0 {
                error!("recv-ed: {}", count / start.elapsed().as_secs().max(1));
                break
            }
        }
    }));
    joins.into_iter().for_each(|j| j.join().unwrap());
}
