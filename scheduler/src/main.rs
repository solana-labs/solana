use log::*;

fn main() {
    solana_logger::setup();
    error!("hello");
    let mut joins = (0..10).map(|_| {
        std::thread::spawn(move || {
            let mut i = 0;
            loop {
                i += 1;
            }
        })
    }).collect::<Vec<_>>();
    joins.push(std::thread::spawn(move || {
    }));
    joins.into_iter().for_each(|j| j.join().unwrap());
}
