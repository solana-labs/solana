use log::trace;
use solana::bank_forks::BankForks;
use solana::blocktree::{get_tmp_ledger_path, Blocktree};
use solana::blocktree_processor::BankForksInfo;
use solana::cluster_info::{ClusterInfo, Node};
use solana::entry::next_entry_mut;
use solana::entry::EntrySlice;
use solana::gossip_service::GossipService;
use solana::leader_scheduler::LeaderScheduler;
use solana::packet::index_blobs;
use solana::rpc_subscriptions::RpcSubscriptions;
use solana::service::Service;
use solana::storage_stage::StorageState;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::streamer;
use solana::tvu::{Sockets, Tvu};
use solana::voting_keypair::VotingKeypair;
use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use std::fs::remove_dir_all;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::time::Duration;

fn new_gossip(
    cluster_info: Arc<RwLock<ClusterInfo>>,
    gossip: UdpSocket,
    exit: Arc<AtomicBool>,
) -> GossipService {
    GossipService::new(&cluster_info, None, None, gossip, exit)
}

/// Test that message sent from leader to target1 and replayed to target2
#[test]
fn test_replay() {
    solana_logger::setup();
    let leader = Node::new_localhost();
    let target1_keypair = Keypair::new();
    let target1 = Node::new_localhost_with_pubkey(target1_keypair.pubkey());
    let target2 = Node::new_localhost();
    let exit = Arc::new(AtomicBool::new(false));

    // start cluster_info_l
    let mut cluster_info_l = ClusterInfo::new(leader.info.clone());
    cluster_info_l.set_leader(leader.info.id);

    let cref_l = Arc::new(RwLock::new(cluster_info_l));
    let dr_l = new_gossip(cref_l, leader.sockets.gossip, exit.clone());

    // start cluster_info2
    let mut cluster_info2 = ClusterInfo::new(target2.info.clone());
    cluster_info2.insert_info(leader.info.clone());
    cluster_info2.set_leader(leader.info.id);
    let cref2 = Arc::new(RwLock::new(cluster_info2));
    let dr_2 = new_gossip(cref2, target2.sockets.gossip, exit.clone());

    // setup some blob services to send blobs into the socket
    // to simulate the source peer and get blobs out of the socket to
    // simulate target peer
    let (s_reader, r_reader) = channel();
    let blob_sockets: Vec<Arc<UdpSocket>> = target2.sockets.tvu.into_iter().map(Arc::new).collect();

    let t_receiver = streamer::blob_receiver(blob_sockets[0].clone(), exit.clone(), s_reader);

    // simulate leader sending messages
    let (s_responder, r_responder) = channel();
    let t_responder = streamer::responder(
        "test_replay",
        Arc::new(leader.sockets.retransmit),
        r_responder,
    );

    let starting_balance = 10_000;
    let (mut genesis_block, mint_keypair) = GenesisBlock::new(starting_balance);

    // TODO: Fix this test so it always works with the default GenesisBlock configuration
    genesis_block.ticks_per_slot = 64;

    let ticks_per_slot = genesis_block.ticks_per_slot;
    let tvu_addr = target1.info.tvu;

    let mut cur_hash = Hash::default();
    let bank_forks = BankForks::new(0, Bank::new(&genesis_block));
    let bank_forks_info = vec![BankForksInfo {
        bank_id: 0,
        entry_height: 0,
        last_entry_id: cur_hash,
        next_blob_index: 0,
    }];

    let bank = bank_forks.working_bank();
    let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new_with_bank(&bank)));
    assert_eq!(bank.get_balance(&mint_keypair.pubkey()), starting_balance);

    // start cluster_info1
    let mut cluster_info1 = ClusterInfo::new(target1.info.clone());
    cluster_info1.insert_info(leader.info.clone());
    cluster_info1.set_leader(leader.info.id);
    let cref1 = Arc::new(RwLock::new(cluster_info1));
    let dr_1 = new_gossip(cref1.clone(), target1.sockets.gossip, exit.clone());

    let blocktree_path = get_tmp_ledger_path("test_replay");

    let (blocktree, ledger_signal_receiver) =
        Blocktree::open_with_config_signal(&blocktree_path, ticks_per_slot)
            .expect("Expected to successfully open ledger");
    let vote_account_keypair = Arc::new(Keypair::new());
    let voting_keypair = VotingKeypair::new_local(&vote_account_keypair);
    let (to_leader_sender, _to_leader_receiver) = channel();
    let tvu = Tvu::new(
        Some(Arc::new(voting_keypair)),
        &Arc::new(RwLock::new(bank_forks)),
        &bank_forks_info,
        &cref1,
        {
            Sockets {
                repair: target1.sockets.repair,
                retransmit: target1.sockets.retransmit,
                fetch: target1.sockets.tvu,
            }
        },
        Arc::new(blocktree),
        STORAGE_ROTATE_TEST_COUNT,
        &to_leader_sender,
        &StorageState::default(),
        None,
        ledger_signal_receiver,
        leader_scheduler,
        &Arc::new(RpcSubscriptions::default()),
    );

    let mut alice_ref_balance = starting_balance;
    let mut msgs = Vec::new();
    let mut blob_idx = 0;
    let num_transfers = 10;
    let transfer_amount = 501;
    let bob_keypair = Keypair::new();
    for i in 0..num_transfers {
        let entry0 = next_entry_mut(&mut cur_hash, i, vec![]);
        let entry_tick0 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

        let tx0 = SystemTransaction::new_account(
            &mint_keypair,
            bob_keypair.pubkey(),
            transfer_amount,
            cur_hash,
            0,
        );
        let entry_tick1 = next_entry_mut(&mut cur_hash, i + 1, vec![]);
        let entry1 = next_entry_mut(&mut cur_hash, i + num_transfers, vec![tx0]);
        let entry_tick2 = next_entry_mut(&mut cur_hash, i + 1, vec![]);

        alice_ref_balance -= transfer_amount;

        let entries = vec![entry0, entry_tick0, entry_tick1, entry1, entry_tick2];
        let blobs = entries.to_shared_blobs();
        index_blobs(&blobs, &mut blob_idx, 0);
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
        trace!("got msg");
    }

    let alice_balance = bank.get_balance(&mint_keypair.pubkey());
    assert_eq!(alice_balance, alice_ref_balance);

    let bob_balance = bank.get_balance(&bob_keypair.pubkey());
    assert_eq!(bob_balance, starting_balance - alice_ref_balance);

    tvu.close().expect("close");
    exit.store(true, Ordering::Relaxed);
    dr_l.join().expect("join");
    dr_2.join().expect("join");
    dr_1.join().expect("join");
    t_receiver.join().expect("join");
    t_responder.join().expect("join");
    Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&blocktree_path);
}
