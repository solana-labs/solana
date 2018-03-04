extern crate silk;

fn main() {
    use silk::accountant_stub::AccountantStub;
    use std::time::Instant;
    use std::net::UdpSocket;
    use silk::event::{generate_keypair, get_pubkey, sign_transaction_data};

    let addr = "127.0.0.1:8000";
    let send_addr = "127.0.0.1:8001";
    let socket = UdpSocket::bind(send_addr).unwrap();
    let acc = AccountantStub::new(addr, socket);
    let alice_keypair = generate_keypair();
    let alice_pubkey = get_pubkey(&alice_keypair);
    let txs = 2_000;
    println!("Depositing {} units in Alice's account...", txs);
    let sig = acc.deposit(txs, &alice_keypair).unwrap();
    acc.wait_on_signature(&sig).unwrap();
    assert_eq!(acc.get_balance(&alice_pubkey).unwrap(), txs);
    println!("Done.");

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
    let mut sig = sig;
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
