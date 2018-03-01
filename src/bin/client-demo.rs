extern crate silk;

fn main() {
    use silk::accountant_stub::AccountantStub;
    use std::thread::sleep;
    use std::time::Duration;
    use silk::log::{generate_keypair, get_pubkey};

    let addr = "127.0.0.1:8000";
    let mut acc = AccountantStub::new(addr);
    let alice_keypair = generate_keypair();
    let bob_keypair = generate_keypair();
    let txs = 10_000;
    println!("Depositing {} units in Alice's account...", txs);
    acc.deposit(txs, &alice_keypair).unwrap();
    //acc.deposit(1_000, &bob_keypair).unwrap();
    println!("Done.");

    sleep(Duration::from_millis(30));
    let alice_pubkey = get_pubkey(&alice_keypair);
    let bob_pubkey = get_pubkey(&bob_keypair);
    println!("Transferring 1 unit {} times...", txs);
    for _ in 0..txs {
        acc.transfer(1, &alice_keypair, bob_pubkey).unwrap();
    }
    println!("Done.");

    sleep(Duration::from_millis(20));
    let mut alice_val = acc.get_balance(&alice_pubkey).unwrap();
    while alice_val > 0 {
        println!("Checking on Alice's Balance {}", alice_val);
        sleep(Duration::from_millis(20));
        alice_val = acc.get_balance(&alice_pubkey).unwrap();
    }
    println!("Done. Checking balances.");
    println!(
        "Alice's Final Balance {}",
        acc.get_balance(&alice_pubkey).unwrap()
    );

    println!(
        "Bob's Final Balance {}",
        acc.get_balance(&bob_pubkey).unwrap()
    );
}
