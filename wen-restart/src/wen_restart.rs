//! The `wen-restart` module handles automatic repair during a cluster restart

use {
    crate::solana::wen_restart_proto::{
        MyLastVotedForkSlots, State as RestartState, WenRestartProgress,
    },
    log::*,
    prost::Message,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::blockstore::Blockstore,
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        fs::File,
        io::{Error, Write},
        path::PathBuf,
        sync::Arc,
    },
};

// The number of ancestor slots sent is hard coded at 81000, because that's
// 400ms * 81000 = 9 hours, we assume most restart decisions to be made in 9
// hours.
const MAX_SLOTS_ON_VOTED_FORKS: u32 = 81000;

pub fn wait_for_wen_restart(
    wen_restart_path: &PathBuf,
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
    cluster_info: Arc<ClusterInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
    // repair and restart option does not work without last voted slot.
    let last_vote_slot = last_vote
        .last_voted_slot()
        .expect("wen_restart doesn't work if local tower is wiped");
    let mut last_vote_fork = vec![last_vote_slot];
    let mut slot = last_vote_slot;
    for _ in 0..MAX_SLOTS_ON_VOTED_FORKS {
        match blockstore.meta(slot) {
            Ok(Some(slot_meta)) => {
                match slot_meta.parent_slot {
                    Some(parent_slot) => {
                        last_vote_fork.push(parent_slot);
                        slot = parent_slot;
                    }
                    None => break,
                };
            }
            _ => break,
        }
    }
    info!(
        "wen_restart last voted fork {} {:?}",
        last_vote_slot, last_vote_fork
    );
    last_vote_fork.reverse();
    // Todo(wen): add the following back in after Gossip code is checked in.
    //    cluster_info.push_last_voted_fork_slots(&last_voted_fork, last_vote.hash());
    // The rest of the protocol will be in another PR.
    let current_progress = WenRestartProgress {
        state: RestartState::Init.into(),
        my_last_voted_fork_slots: Some(MyLastVotedForkSlots {
            last_vote_slot,
            last_vote_bankhash: last_vote.hash().to_string(),
            shred_version: cluster_info.my_shred_version() as u32,
        }),
    };
    write_wen_restart_records(wen_restart_path, current_progress)?;
    Ok(())
}

fn write_wen_restart_records(
    records_path: &PathBuf,
    new_progress: WenRestartProgress,
) -> Result<(), Error> {
    // overwrite anything if exists
    let mut file = File::create(records_path)?;
    info!("writing new record {:?}", new_progress);
    let mut buf = Vec::with_capacity(new_progress.encoded_len());
    new_progress.encode(&mut buf)?;
    file.write_all(&buf)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use {
        crate::wen_restart::*,
        solana_entry::entry,
        solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
        solana_ledger::{blockstore, get_tmp_ledger_path_auto_delete},
        solana_program::{hash::Hash, vote::state::Vote},
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{fs::read, sync::Arc},
    };

    #[test]
    fn test_wen_restart_normal_flow() {
        solana_logger::setup();
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = Arc::new(ClusterInfo::new(
            {
                let mut contact_info =
                    ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp());
                contact_info.set_shred_version(2);
                contact_info
            },
            node_keypair,
            SocketAddrSpace::Unspecified,
        ));
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let mut wen_restart_proto_path = ledger_path.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let blockstore = Arc::new(blockstore::Blockstore::open(ledger_path.path()).unwrap());
        let last_vote_slot = 400;
        for i in 0..last_vote_slot {
            let entries = entry::create_ticks(1, 0, Hash::default());
            let shreds = blockstore::entries_to_test_shreds(
                &entries,
                i + 1,
                i,
                false,
                0,
                true, // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
        }
        let last_vote_bankhash = Hash::new_unique();
        assert!(wait_for_wen_restart(
            &wen_restart_proto_path,
            VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
            blockstore,
            cluster_info
        )
        .is_ok());
        let buffer = read(wen_restart_proto_path).unwrap();
        let progress = WenRestartProgress::decode(&mut std::io::Cursor::new(buffer)).unwrap();
        assert_eq!(
            progress,
            WenRestartProgress {
                state: RestartState::Init.into(),
                my_last_voted_fork_slots: Some(MyLastVotedForkSlots {
                    last_vote_slot,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: 2,
                }),
            }
        )
    }
}
