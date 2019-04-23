#[macro_use]
extern crate solana;

use log::*;
use solana::banking_stage::create_test_recorder;
use solana::blocktree::{create_new_tmp_ledger, Blocktree};
use solana::cluster_info::{ClusterInfo, Node};
use solana::entry::next_entry_mut;
use solana::entry::EntrySlice;
use solana::fullnode;
use solana::gossip_service::GossipService;
use solana::packet::index_blobs;
use solana::rpc_subscriptions::RpcSubscriptions;
use solana::service::Service;
use solana::storage_stage::StorageState;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::streamer;
use solana::tvu::{Sockets, Tvu};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction;
use std::fs::remove_dir_all;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::time::Duration;

fn new_gossip(
    cluster_info: Arc<RwLock<ClusterInfo>>,
    gossip: UdpSocket,
    exit: &Arc<AtomicBool>,
) -> GossipService {
    GossipService::new(&cluster_info, None, None, gossip, exit)
}

/// Test that message sent from leader to target1 and replayed to target2
#[test]
fn test_replay() {
    solana_logger::setup();
    let leader = Node::new_localhost();
    let target1_keypair = Keypair::new();
    let target1 = Node::new_localhost_with_pubkey(&target1_keypair.pubkey());
    let target2 = Node::new_localhost();
    let exit = Arc::new(AtomicBool::new(false));

    // start cluster_info_l
    let cluster_info_l = ClusterInfo::new_with_invalid_keypair(leader.info.clone());

    let cref_l = Arc::new(RwLock::new(cluster_info_l));
    let dr_l = new_gossip(cref_l, leader.sockets.gossip, &exit);

    // start cluster_info2
    let mut cluster_info2 = ClusterInfo::new_with_invalid_keypair(target2.info.clone());
    cluster_info2.insert_info(leader.info.clone());
    let cref2 = Arc::new(RwLock::new(cluster_info2));
    let dr_2 = new_gossip(cref2, target2.sockets.gossip, &exit);

    // setup some blob services to send blobs into the socket
    // to simulate the source peer and get blobs out of the socket to
    // simulate target peer
    let (s_reader, r_reader) = channel();
    let blob_sockets: Vec<Arc<UdpSocket>> = target2.sockets.tvu.into_iter().map(Arc::new).collect();

    let t_receiver = streamer::blob_receiver(blob_sockets[0].clone(), &exit, s_reader);

    // simulate leader sending messages
    let (s_responder, r_responder) = channel();
    let t_responder = streamer::responder(
        "test_replay",
        Arc::new(leader.sockets.retransmit),
        r_responder,
    );

    let total_balance = 10_000;
    let leader_balance = 100;
    let starting_mint_balance = total_balance - leader_balance;
    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(total_balance, &leader.info.id, leader_balance);
    genesis_block.ticks_per_slot = 160;
    genesis_block.slots_per_epoch = 16;
    let (blocktree_path, blockhash) = create_new_tmp_ledger!(&genesis_block);

    let tvu_addr = target1.info.tvu;

    let (bank_forks, bank_forks_info, blocktree, ledger_signal_receiver, leader_schedule_cache) =
        fullnode::new_banks_from_blocktree(&blocktree_path, None);
    let bank = bank_forks.working_bank();
    assert_eq!(
        bank.get_balance(&mint_keypair.pubkey()),
        starting_mint_balance
    );

    let leader_schedule_cache = Arc::new(leader_schedule_cache);
    // start cluster_info1
    let bank_forks = Arc::new(RwLock::new(bank_forks));
    let mut cluster_info1 = ClusterInfo::new_with_invalid_keypair(target1.info.clone());
    cluster_info1.insert_info(leader.info.clone());
    let cref1 = Arc::new(RwLock::new(cluster_info1));
    let dr_1 = new_gossip(cref1.clone(), target1.sockets.gossip, &exit);

    let voting_keypair = Keypair::new();
    let blocktree = Arc::new(blocktree);
    {
        let (poh_service_exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(&bank, &blocktree);
        let (storage_sender, storage_receiver) = channel();
        let tvu = Tvu::new(
            &voting_keypair.pubkey(),
            Some(Arc::new(voting_keypair)),
            &bank_forks,
            &bank_forks_info,
            &cref1,
            {
                Sockets {
                    repair: target1.sockets.repair,
                    retransmit: target1.sockets.retransmit,
                    fetch: target1.sockets.tvu,
                }
            },
            blocktree,
            STORAGE_ROTATE_TEST_COUNT,
            &StorageState::default(),
            None,
            ledger_signal_receiver,
            &Arc::new(RpcSubscriptions::default()),
            &poh_recorder,
            storage_sender,
            storage_receiver,
            &leader_schedule_cache,
            &exit,
            &solana_sdk::hash::Hash::default(),
        );

        let mut mint_ref_balance = starting_mint_balance;
        let mut msgs = Vec::new();
        let mut blob_idx = 0;
        let num_transfers = 10;
        let mut transfer_amount = 501;
        let bob_keypair = Keypair::new();
        let mut cur_hash = blockhash;
        for i in 0..num_transfers {
            let entry0 = next_entry_mut(&mut cur_hash, i, vec![]);
            let entry_tick0 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

            let tx0 = system_transaction::create_user_account(
                &mint_keypair,
                &bob_keypair.pubkey(),
                transfer_amount,
                blockhash,
                0,
            );
            let entry_tick1 = next_entry_mut(&mut cur_hash, i + 1, vec![]);
            let entry1 = next_entry_mut(&mut cur_hash, i + num_transfers, vec![tx0]);
            let entry_tick2 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

            mint_ref_balance -= transfer_amount;
            transfer_amount -= 1; // Sneaky: change transfer_amount slightly to avoid DuplicateSignature errors

            let entries = vec![entry0, entry_tick0, entry_tick1, entry1, entry_tick2];
            let blobs = entries.to_shared_blobs();
            index_blobs(&blobs, &leader.info.id, blob_idx, 1, 0);
            blob_idx += blobs.len() as u64;
            blobs
                .iter()
                .for_each(|b| b.write().unwrap().meta.set_addr(&tvu_addr));
            msgs.extend(blobs.into_iter());
        }

        // send the blobs into the socket
        s_responder.send(msgs).expect("send");
        drop(s_responder);

        // receive retransmitted messages
        let timer = Duration::new(1, 0);
        while let Ok(_msg) = r_reader.recv_timeout(timer) {
            info!("got msg");
        }

        let working_bank = bank_forks.read().unwrap().working_bank();
        let final_mint_balance = working_bank.get_balance(&mint_keypair.pubkey());
        assert_eq!(final_mint_balance, mint_ref_balance);

        let bob_balance = working_bank.get_balance(&bob_keypair.pubkey());
        assert_eq!(bob_balance, starting_mint_balance - mint_ref_balance);

        exit.store(true, Ordering::Relaxed);
        poh_service_exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
        tvu.join().unwrap();
        dr_l.join().unwrap();
        dr_2.join().unwrap();
        dr_1.join().unwrap();
        t_receiver.join().unwrap();
        t_responder.join().unwrap();
    }
    Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&blocktree_path);
}
