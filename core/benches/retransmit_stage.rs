#![feature(test)]

extern crate solana_core;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    solana_core::retransmit_stage::retransmitter,
    solana_entry::entry::Entry,
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::ContactInfo,
    },
    solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        leader_schedule_cache::LeaderScheduleCache,
        shred::{ProcessShredsStats, Shredder},
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
    std::{
        net::UdpSocket,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, RwLock,
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
#[allow(clippy::same_item_push)]
fn bench_retransmitter(bencher: &mut Bencher) {
    solana_logger::setup();
    let cluster_info = ClusterInfo::new(
        Node::new_localhost().info,
        Arc::new(Keypair::new()),
        SocketAddrSpace::Unspecified,
    );
    const NUM_PEERS: usize = 4;
    let mut peer_sockets = Vec::new();
    for _ in 0..NUM_PEERS {
        let id = Pubkey::new_unique();
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut contact_info = ContactInfo::new_localhost(&id, timestamp());
        contact_info.tvu = socket.local_addr().unwrap();
        contact_info.tvu.set_ip("127.0.0.1".parse().unwrap());
        contact_info.tvu_forwards = contact_info.tvu;
        info!("local: {:?}", contact_info.tvu);
        cluster_info.insert_info(contact_info);
        socket.set_nonblocking(true).unwrap();
        peer_sockets.push(socket);
    }
    let peer_sockets = Arc::new(peer_sockets);
    let cluster_info = Arc::new(cluster_info);

    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
    let bank0 = Bank::new_for_benches(&genesis_config);
    let bank_forks = BankForks::new(bank0);
    let bank = bank_forks.working_bank();
    let bank_forks = Arc::new(RwLock::new(bank_forks));
    let (shreds_sender, shreds_receiver) = unbounded();
    const NUM_THREADS: usize = 2;
    let sockets = (0..NUM_THREADS)
        .map(|_| UdpSocket::bind("0.0.0.0:0").unwrap())
        .collect();

    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

    // To work reliably with higher values, this needs larger udp rmem size
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
    let slot = 0;
    let parent = 0;
    let shredder = Shredder::new(slot, parent, 0, 0).unwrap();
    let (mut data_shreds, _) = shredder.entries_to_shreds(
        &keypair,
        &entries,
        true, // is_last_in_slot
        0,    // next_shred_index
        0,    // next_code_index
        &mut ProcessShredsStats::default(),
    );

    let num_packets = data_shreds.len();

    let retransmitter_handles = retransmitter(
        Arc::new(sockets),
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
