#![feature(test)]

extern crate solana_turbine;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    solana_entry::entry::Entry,
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::{ContactInfo, Protocol},
    },
    solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        leader_schedule_cache::LeaderScheduleCache,
        shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
    },
    solana_measure::measure::Measure,
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_transaction,
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_turbine::retransmit_stage::retransmitter,
    std::{
        iter::repeat_with,
        net::{Ipv4Addr, UdpSocket},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread::{sleep, Builder},
        time::Duration,
    },
    test::Bencher,
};

// TODO: The benchmark is ignored as it currently may indefinitely block.
// The code incorrectly expects that the node receiving the shred on tvu socket
// retransmits that to other nodes in its neighborhood. But that is no longer
// the case since https://github.com/solana-labs/solana/pull/17716.
// So depending on shred seed, peers may not receive packets and the receive
// threads loop indefinitely.
#[ignore]
#[bench]
fn bench_retransmitter(bencher: &mut Bencher) {
    solana_logger::setup();
    let cluster_info = {
        let keypair = Arc::new(Keypair::new());
        let node = Node::new_localhost_with_pubkey(&keypair.pubkey());
        ClusterInfo::new(node.info, keypair, SocketAddrSpace::Unspecified)
    };
    const NUM_PEERS: usize = 4;
    let peer_sockets: Vec<_> = repeat_with(|| {
        let id = Pubkey::new_unique();
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut contact_info = ContactInfo::new_localhost(&id, timestamp());
        let port = socket.local_addr().unwrap().port();
        contact_info.set_tvu((Ipv4Addr::LOCALHOST, port)).unwrap();
        info!("local: {:?}", contact_info.tvu(Protocol::UDP).unwrap());
        cluster_info.insert_info(contact_info);
        socket.set_nonblocking(true).unwrap();
        socket
    })
    .take(NUM_PEERS)
    .collect();
    let peer_sockets = Arc::new(peer_sockets);
    let cluster_info = Arc::new(cluster_info);

    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
    let bank0 = Bank::new_for_benches(&genesis_config);
    let bank_forks = BankForks::new_rw_arc(bank0);
    let bank = bank_forks.read().unwrap().working_bank();
    let (shreds_sender, shreds_receiver) = unbounded();
    const NUM_THREADS: usize = 2;
    let sockets = (0..NUM_THREADS)
        .map(|_| UdpSocket::bind("0.0.0.0:0").unwrap())
        .collect();

    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

    // To work reliably with higher values, this needs larger udp mem size
    let entries: Vec<_> = (0..5)
        .map(|_| {
            let keypair0 = Keypair::new();
            let keypair1 = Keypair::new();
            let tx0 =
                system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
            Entry::new(&Hash::default(), 1, vec![tx0])
        })
        .collect();

    let keypair = Keypair::new();
    let (quic_endpoint_sender, _quic_endpoint_receiver) =
        tokio::sync::mpsc::channel(/*capacity:*/ 128);
    let slot = 0;
    let parent = 0;
    let shredder = Shredder::new(slot, parent, 0, 0).unwrap();
    let (mut data_shreds, _) = shredder.entries_to_shreds(
        &keypair,
        &entries,
        true, // is_last_in_slot
        0,    // next_shred_index
        0,    // next_code_index
        true, // merkle_variant
        &ReedSolomonCache::default(),
        &mut ProcessShredsStats::default(),
    );

    let num_packets = data_shreds.len();

    let retransmitter_handles = retransmitter(
        Arc::new(sockets),
        quic_endpoint_sender,
        bank_forks,
        leader_schedule_cache,
        cluster_info,
        shreds_receiver,
        Arc::default(), // solana_rpc::max_slots::MaxSlots
        None,
    );

    let mut index = 0;
    let mut slot = 0;
    let total = Arc::new(AtomicUsize::new(0));
    bencher.iter(move || {
        let peer_sockets1 = peer_sockets.clone();
        let handles: Vec<_> = (0..NUM_PEERS)
            .map(|p| {
                let peer_sockets2 = peer_sockets1.clone();
                let total2 = total.clone();
                Builder::new()
                    .name("recv".to_string())
                    .spawn(move || {
                        info!("{} waiting on {:?}", p, peer_sockets2[p]);
                        let mut buf = [0u8; 1024];
                        loop {
                            while peer_sockets2[p].recv(&mut buf).is_ok() {
                                total2.fetch_add(1, Ordering::Relaxed);
                            }
                            if total2.load(Ordering::Relaxed) >= num_packets {
                                break;
                            }
                            info!("{} recv", total2.load(Ordering::Relaxed));
                            sleep(Duration::from_millis(1));
                        }
                    })
                    .unwrap()
            })
            .collect();

        for shred in data_shreds.iter_mut() {
            shred.set_slot(slot);
            shred.set_index(index);
            index += 1;
            index %= 200;
            let shred = shred.payload().clone();
            let _ = shreds_sender.send(vec![shred]);
        }
        slot += 1;

        info!("sent...");

        let mut join_time = Measure::start("join");
        for h in handles {
            h.join().unwrap();
        }
        join_time.stop();
        info!("took: {}ms", join_time.as_ms());

        total.store(0, Ordering::Relaxed);
    });

    retransmitter_handles.join().unwrap();
}
