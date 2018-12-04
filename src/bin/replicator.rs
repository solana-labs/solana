#[macro_use]
extern crate clap;
extern crate getopts;
extern crate serde_json;
#[macro_use]
extern crate solana;
extern crate solana_drone;
extern crate solana_sdk;

use clap::{App, Arg};
use solana::chacha::{chacha_cbc_encrypt_file, CHACHA_BLOCK_SIZE};
use solana::client::mk_client;
use solana::cluster_info::Node;
use solana::fullnode::Config;
use solana::ledger::LEDGER_DATA_FILE;
use solana::logger;
use solana::replicator::{sample_file, Replicator};
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::storage_program::StorageTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    logger::setup();

    let matches = App::new("replicator")
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("Run with the identity found in FILE"),
        ).arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point"),
        ).arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR as persistent ledger location"),
        ).get_matches();

    let ledger_path = matches.value_of("ledger");

    let (keypair, ncp) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                (data.keypair(), data.node_info.ncp)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!([127, 0, 0, 1], 8700))
    };

    let node = Node::new_with_external_ip(keypair.pubkey(), &ncp);

    println!(
        "replicating the data with keypair: {:?} ncp:{:?}",
        keypair.pubkey(),
        ncp
    );

    let exit = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    let network_addr = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"));

    // TODO: ask network what slice we should store
    let entry_height = 0;

    let (replicator, leader_info) = Replicator::new(
        entry_height,
        5,
        &exit,
        ledger_path,
        node,
        network_addr,
        done.clone(),
    );

    while !done.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(100));
    }

    println!("Done downloading ledger");

    let ledger_path = Path::new(ledger_path.unwrap());
    let ledger_data_file = ledger_path.join(LEDGER_DATA_FILE);
    let ledger_data_file_encrypted = ledger_path.join(format!("{}.enc", LEDGER_DATA_FILE));
    let mut ivec = [0u8; CHACHA_BLOCK_SIZE];
    ivec[0..4].copy_from_slice(&[2, 3, 4, 5]);

    if let Err(e) =
        chacha_cbc_encrypt_file(&ledger_data_file, &ledger_data_file_encrypted, &mut ivec)
    {
        println!("Error while encrypting ledger: {:?}", e);
        return;
    }

    println!("Done encrypting the ledger");

    let sampling_offsets = [0, 1, 2, 3];

    let mut client = mk_client(&leader_info);

    let mut drone_addr = leader_info.tpu;
    drone_addr.set_port(DRONE_PORT);
    let airdrop_amount = 5;
    let last_id = client.get_last_id();
    let transaction =
        request_airdrop_transaction(&drone_addr, &keypair.pubkey(), airdrop_amount, last_id)
            .unwrap();
    let signature = client.transfer_signed(&transaction).unwrap();
    client.poll_for_signature(&signature).unwrap();

    match sample_file(&ledger_data_file_encrypted, &sampling_offsets) {
        Ok(hash) => {
            let last_id = client.get_last_id();
            println!("sampled hash: {}", hash);
            let tx = Transaction::storage_new_mining_proof(&keypair, hash, last_id);
            client.transfer_signed(&tx).expect("transfer didn't work!");
        }
        Err(e) => println!("Error occurred while sampling: {:?}", e),
    }

    replicator.join();
}
