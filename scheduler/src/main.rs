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
    let (s2, r2) = bounded(20);

    let mut joins = (0..3).map(|thx| {
        let s = s.clone();
        std::thread::spawn(move || {
            let mut i = 0;
            //for _ in 0..6000000 {
            loop {
                let ss = r2.recv().unwrap();
                s.send(ss).unwrap((thx, i, ss));
                i += 1;
            }
        })
    }).collect::<Vec<_>>();

    joins.push(std::thread::spawn(move || {
        loop {
            s2.send((std::time::Instant::now(), ExecutionEnvironment::default())).unwrap();
        }
    }));

    joins.push(std::thread::spawn(move || {
        let mut count = 0;
        let start = std::time::Instant::now();
        //let mut rrr = Vec::with_capacity(10);
        //for _ in 0..10 {
        loop {
            let rr = r.recv().unwrap();
            //rrr.push((rr.2.elapsed(), rr));
        //}

        //for rr in rrr {
            count += 1;
            //error!("recv-ed: {:?}", &rr);
            if count % 1_000_000 == 0 {
                error!("recv-ed: {}", count / start.elapsed().as_secs().max(1));
                //break
            }
        }
    }));
    joins.into_iter().for_each(|j| j.join().unwrap());
}
