//extern crate serde_json;
extern crate silk;

//use std::io::stdin;

fn main() {
    use silk::accountant_stub::AccountantStub;
    use silk::accountant_skel::AccountantSkel;
    use silk::accountant::Accountant;
    use silk::event::{generate_keypair, get_pubkey, sign_transaction_data};
    use silk::genesis::Genesis;
    use std::time::Instant;
    use std::net::UdpSocket;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    let addr = "127.0.0.1:8000";
    let send_addr = "127.0.0.1:8001";

    let txs = 200;

    let gen = Genesis::new(txs, vec![]);
    let alice_keypair = generate_keypair();
    //let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    //let alice_keypair = gen.get_keypair();

    let acc = Accountant::new(&gen, None);
    spawn(move || AccountantSkel::new(acc).serve(addr).unwrap());
    sleep(Duration::from_millis(30));

    let socket = UdpSocket::bind(send_addr).unwrap();
    let acc = AccountantStub::new(addr, socket);
    let alice_pubkey = get_pubkey(&alice_keypair);
    let one = 1;
    println!("Signing transactions...");
    let now = Instant::now();
    let sigs: Vec<(_, _)> = (0..txs)
        .map(|_| {
            let rando_keypair = generate_keypair();
            let rando_pubkey = get_pubkey(&rando_keypair);
            let sig = sign_transaction_data(&one, &alice_keypair, &rando_pubkey);
            (rando_pubkey, sig)
        })
        .collect();
    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let bsps = txs as f64 / ns as f64;
    let nsps = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    println!("Verify signatures...");
    use silk::event::{verify_event, Event};
    let now = Instant::now();
    for &(k, s) in &sigs {
        let e = Event::Transaction {
            from: alice_pubkey,
            to: k,
            data: one,
            sig: s,
        };
        assert!(verify_event(&e));
    }
    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let bsvps = txs as f64 / ns as f64;
    let nspsv = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signature verifications per second, {}us per signature verification",
        bsvps * 1_000_000_f64,
        nspsv / 1_000_f64
    );

    println!("Transferring 1 unit {} times...", txs);
    let now = Instant::now();
    let mut sig = Default::default();
    for (k, s) in sigs {
        acc.transfer_signed(alice_pubkey, k, one, s).unwrap();
        sig = s;
    }
    println!("Waiting for last transaction to be confirmed...",);
    acc.wait_on_signature(&sig).unwrap();

    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let tps = (txs * 1_000_000_000) as f64 / ns as f64;
    println!("Done. {} tps!", tps);
    let val = acc.get_balance(&alice_pubkey).unwrap();
    println!("Alice's Final Balance {}", val);
    assert_eq!(val, 0);
}
