use log::*;
use rand::Rng;

fn main() {
    solana_logger::setup();

    //let (s, r) = crossbeam_channel::unbounded();
    let (s, r) = crossbeam_channel::bounded(100_000);
    //let (s, r) = flume::unbounded();
    //let (s, r) = flume::bounded(100_000);

    std::thread::scope(|scope| {
        std::thread::Builder::new().name("send".into()).spawn_scoped(scope, || {
            let mut t = 0;

            loop {
                for i in 0..10_000_000 {
                    t += rand::thread_rng().gen::<u64>();
                }
                info!("sent begin");
                //s.send_buffered(()).unwrap();
                s.send_buffered(()).unwrap();
                info!("sent end");
            }
            info!("{}", t);
        }).unwrap();

        std::thread::Builder::new().name("recv".into()).spawn_scoped(scope, || {
            loop {
                /*if let Ok(u) = r.try_recv() {
                    info!("recv");
                }*/
                if let Ok(u) = r.recv() {
                    info!("recv");
                }
            }
        }).unwrap();
    });
}
