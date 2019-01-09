//! Set of functions for emulating windowing functions from a database ledger implementation
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::db_ledger::*;
use crate::entry::Entry;
#[cfg(feature = "erasure")]
use crate::erasure;
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::{SharedBlob, BLOB_HEADER_SIZE};
use crate::result::Result;
use crate::streamer::BlobSender;
use log::Level;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use std::borrow::Borrow;
use std::cmp;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

pub const MAX_REPAIR_LENGTH: usize = 128;

pub fn repair(
    db_ledger: &DbLedger,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    id: &Pubkey,
    times: usize,
    tick_height: u64,
    max_entry_height: u64,
    leader_scheduler_option: &Arc<RwLock<LeaderScheduler>>,
) -> Result<Vec<(SocketAddr, Vec<u8>)>> {
    let rcluster_info = cluster_info.read().unwrap();
    let mut is_next_leader = false;
    let meta = db_ledger.meta()?;
    if meta.is_none() {
        return Ok(vec![]);
    }
    let meta = meta.unwrap();

    let consumed = meta.consumed;
    let received = meta.received;

    // Repair should only be called when received > consumed, enforced in window_service
    assert!(received > consumed);
    {
        let ls_lock = leader_scheduler_option.read().unwrap();
        if !ls_lock.use_only_bootstrap_leader {
            // Calculate the next leader rotation height and check if we are the leader
            if let Some(next_leader_rotation_height) = ls_lock.max_height_for_leader(tick_height) {
                match ls_lock.get_scheduled_leader(next_leader_rotation_height) {
                    Some((leader_id, _)) if leader_id == *id => is_next_leader = true,
                    // In the case that we are not in the current scope of the leader schedule
                    // window then either:
                    //
                    // 1) The replay stage hasn't caught up to the "consumed" entries we sent,
                    // in which case it will eventually catch up
                    //
                    // 2) We are on the border between seed_rotation_intervals, so the
                    // schedule won't be known until the entry on that cusp is received
                    // by the replay stage (which comes after this stage). Hence, the next
                    // leader at the beginning of that next epoch will not know they are the
                    // leader until they receive that last "cusp" entry. The leader also won't ask for repairs
                    // for that entry because "is_next_leader" won't be set here. In this case,
                    // everybody will be blocking waiting for that "cusp" entry instead of repairing,
                    // until the leader hits "times" >= the max times in calculate_max_repair_entry_height().
                    // The impact of this, along with the similar problem from broadcast for the transitioning
                    // leader, can be observed in the multinode test, test_full_leader_validator_network(),
                    None => (),
                    _ => (),
                }
            }
        }
    }

    let num_peers = rcluster_info.repair_peers().len() as u64;

    // Check if there's a max_entry_height limitation
    let max_repair_entry_height = if max_entry_height == 0 {
        calculate_max_repair_entry_height(num_peers, consumed, received, times, is_next_leader)
    } else {
        max_entry_height + 2
    };

    let idxs = db_ledger.find_missing_data_indexes(
        DEFAULT_SLOT_HEIGHT,
        consumed,
        max_repair_entry_height - 1,
        MAX_REPAIR_LENGTH,
    );

    let reqs: Vec<_> = idxs
        .into_iter()
        .filter_map(|pix| rcluster_info.window_index_request(pix).ok())
        .collect();

    drop(rcluster_info);

    inc_new_counter_info!("streamer-repair_window-repair", reqs.len());

    if log_enabled!(Level::Trace) {
        trace!(
            "{}: repair_window counter times: {} consumed: {} received: {} max_repair_entry_height: {} missing: {}",
            id,
            times,
            consumed,
            received,
            max_repair_entry_height,
            reqs.len()
        );
        for (to, _) in &reqs {
            trace!("{}: repair_window request to {}", id, to);
        }
    }

    Ok(reqs)
}

pub fn retransmit_all_leader_blocks(
    dq: &[SharedBlob],
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
    retransmit: &BlobSender,
) -> Result<()> {
    let mut retransmit_queue: Vec<SharedBlob> = Vec::new();
    for b in dq {
        // Check if the blob is from the scheduled leader for its slot. If so,
        // add to the retransmit_queue
        if let Ok(slot) = b.read().unwrap().slot() {
            if let Some(leader_id) = leader_scheduler.read().unwrap().get_leader_for_slot(slot) {
                add_blob_to_retransmit_queue(b, leader_id, &mut retransmit_queue);
            }
        }
    }

    submit(
        influxdb::Point::new("retransmit-queue")
            .add_field(
                "count",
                influxdb::Value::Integer(retransmit_queue.len() as i64),
            )
            .to_owned(),
    );

    if !retransmit_queue.is_empty() {
        inc_new_counter_info!("streamer-recv_window-retransmit", retransmit_queue.len());
        retransmit.send(retransmit_queue)?;
    }
    Ok(())
}

pub fn add_blob_to_retransmit_queue(
    b: &SharedBlob,
    leader_id: Pubkey,
    retransmit_queue: &mut Vec<SharedBlob>,
) {
    let p = b.read().unwrap();
    if p.id().expect("get_id in fn add_block_to_retransmit_queue") == leader_id {
        let nv = SharedBlob::default();
        {
            let mut mnv = nv.write().unwrap();
            let sz = p.meta.size;
            mnv.meta.size = sz;
            mnv.data[..sz].copy_from_slice(&p.data[..sz]);
        }
        retransmit_queue.push(nv);
    }
}

/// Process a blob: Add blob to the ledger window. If a continuous set of blobs
/// starting from consumed is thereby formed, add that continuous
/// range of blobs to a queue to be sent on to the next stage.
pub fn process_blob(
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
    db_ledger: &Arc<DbLedger>,
    blob: &SharedBlob,
    max_ix: u64,
    consume_queue: &mut Vec<Entry>,
    tick_height: &mut u64,
    done: &Arc<AtomicBool>,
) -> Result<()> {
    let is_coding = blob.read().unwrap().is_coding();

    // Check if the blob is in the range of our known leaders. If not, we return.
    // TODO: Need to update slot in broadcast, otherwise this check will fail with
    // leader rotation enabled
    // Github issue: https://github.com/solana-labs/solana/issues/1899.
    let (slot, pix) = {
        let r_blob = blob.read().unwrap();
        (r_blob.slot()?, r_blob.index()?)
    };
    let leader = leader_scheduler.read().unwrap().get_leader_for_slot(slot);

    // TODO: Once the original leader signature is added to the blob, make sure that
    // the blob was originally generated by the expected leader for this slot
    if leader.is_none() {
        return Ok(());
    }

    // Insert the new blob into the window
    let mut consumed_entries = if is_coding {
        let blob = &blob.read().unwrap();
        db_ledger.put_coding_blob_bytes(
            slot,
            pix,
            &blob.data[..BLOB_HEADER_SIZE + blob.size().unwrap()],
        )?;
        vec![]
    } else {
        db_ledger.insert_data_blobs(vec![(*blob.read().unwrap()).borrow()])?
    };

    #[cfg(feature = "erasure")]
    {
        // If write_shared_blobs() of these recovered blobs fails fails, don't return
        // because consumed_entries might be nonempty from earlier, and tick height needs to
        // be updated. Hopefully we can recover these blobs next time successfully.
        if let Err(e) = try_erasure(db_ledger, &mut consumed_entries) {
            trace!(
                "erasure::recover failed to write recovered coding blobs. Err: {:?}",
                e
            );
        }
    }

    for entry in &consumed_entries {
        *tick_height += entry.is_tick() as u64;
    }

    // For downloading storage blobs,
    // we only want up to a certain index
    // then stop
    if max_ix != 0 && !consumed_entries.is_empty() {
        let meta = db_ledger
            .meta()?
            .expect("Expect metadata to exist if consumed entries is nonzero");

        let consumed = meta.consumed;

        // Check if we ran over the last wanted entry
        if consumed > max_ix {
            let consumed_entries_len = consumed_entries.len();
            let extra_unwanted_entries_len =
                cmp::min(consumed_entries_len, (consumed - (max_ix + 1)) as usize);
            consumed_entries.truncate(consumed_entries_len - extra_unwanted_entries_len);
            done.store(true, Ordering::Relaxed);
        }
    }

    consume_queue.extend(consumed_entries);
    Ok(())
}

pub fn calculate_max_repair_entry_height(
    num_peers: u64,
    consumed: u64,
    received: u64,
    times: usize,
    is_next_leader: bool,
) -> u64 {
    // Calculate the highest blob index that this node should have already received
    // via avalanche. The avalanche splits data stream into nodes and each node retransmits
    // the data to their peer nodes. So there's a possibility that a blob (with index lower
    // than current received index) is being retransmitted by a peer node.
    if times >= 8 || is_next_leader {
        // if repair backoff is getting high, or if we are the next leader,
        // don't wait for avalanche. received - 1 is the index of the highest blob.
        received
    } else {
        cmp::max(consumed, received.saturating_sub(num_peers))
    }
}

#[cfg(feature = "erasure")]
fn try_erasure(db_ledger: &Arc<DbLedger>, consume_queue: &mut Vec<Entry>) -> Result<()> {
    let meta = db_ledger.meta()?;

    if let Some(meta) = meta {
        let (data, coding) = erasure::recover(db_ledger, meta.consumed_slot, meta.consumed)?;
        for c in coding {
            let c = c.read().unwrap();
            db_ledger.put_coding_blob_bytes(
                meta.consumed_slot,
                c.index().unwrap(),
                &c.data[..BLOB_HEADER_SIZE + c.size().unwrap()],
            )?;
        }

        let entries = db_ledger.write_shared_blobs(data)?;
        consume_queue.extend(entries);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[cfg(all(feature = "erasure", test))]
    use crate::entry::reconstruct_entries_from_blobs;
    #[cfg(all(feature = "erasure", test))]
    use crate::erasure::test::{generate_db_ledger_from_window, setup_window_ledger};
    #[cfg(all(feature = "erasure", test))]
    use crate::erasure::{NUM_CODING, NUM_DATA};
    use crate::ledger::{get_tmp_ledger_path, make_tiny_test_entries, EntrySlice};
    use crate::packet::{index_blobs, Blob, Packet, Packets, SharedBlob, PACKET_DATA_SIZE};
    use crate::streamer::{receiver, responder, PacketReceiver};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn get_msgs(r: PacketReceiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.read().unwrap().packets.len(),
                e => info!("error {:?}", e),
            }
            if *num == 10 {
                break;
            }
        }
    }
    #[test]
    pub fn streamer_debug() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
        write!(io::sink(), "{:?}", Blob::default()).unwrap();
    }

    #[test]
    pub fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(
            Arc::new(read),
            exit.clone(),
            s_reader,
            "window-streamer-test",
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder("streamer_send_test", Arc::new(send), r_responder);
            let mut msgs = Vec::new();
            for i in 0..10 {
                let b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.data[0] = i as u8;
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&addr);
                }
                msgs.push(b);
            }
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }

    #[test]
    pub fn test_calculate_max_repair_entry_height() {
        assert_eq!(calculate_max_repair_entry_height(20, 4, 11, 0, false), 4);
        assert_eq!(calculate_max_repair_entry_height(0, 10, 90, 0, false), 90);
        assert_eq!(calculate_max_repair_entry_height(15, 10, 90, 32, false), 90);
        assert_eq!(calculate_max_repair_entry_height(15, 10, 90, 0, false), 75);
        assert_eq!(calculate_max_repair_entry_height(90, 10, 90, 0, false), 10);
        assert_eq!(calculate_max_repair_entry_height(90, 10, 50, 0, false), 10);
        assert_eq!(calculate_max_repair_entry_height(90, 10, 99, 0, false), 10);
        assert_eq!(calculate_max_repair_entry_height(90, 10, 101, 0, false), 11);
        assert_eq!(calculate_max_repair_entry_height(90, 10, 101, 0, true), 101);
        assert_eq!(
            calculate_max_repair_entry_height(90, 10, 101, 30, true),
            101
        );
    }

    #[test]
    pub fn test_retransmit() {
        let leader = Keypair::new().pubkey();
        let nonleader = Keypair::new().pubkey();
        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(leader)));
        let blob = SharedBlob::default();

        let (blob_sender, blob_receiver) = channel();

        // Expect blob from leader to be retransmitted
        blob.write().unwrap().set_id(&leader).unwrap();
        retransmit_all_leader_blocks(&vec![blob.clone()], &leader_scheduler, &blob_sender)
            .expect("Expect successful retransmit");
        let output_blob = blob_receiver
            .try_recv()
            .expect("Expect input blob to be retransmitted");

        // Retransmitted blob should be missing the leader id
        assert_ne!(*output_blob[0].read().unwrap(), *blob.read().unwrap());
        // Set the leader in the retransmitted blob, should now match the original
        output_blob[0].write().unwrap().set_id(&leader).unwrap();
        assert_eq!(*output_blob[0].read().unwrap(), *blob.read().unwrap());

        // Expect blob from nonleader to not be retransmitted
        blob.write().unwrap().set_id(&nonleader).unwrap();
        retransmit_all_leader_blocks(&vec![blob], &leader_scheduler, &blob_sender)
            .expect("Expect successful retransmit");
        assert!(blob_receiver.try_recv().is_err());
    }

    #[test]
    pub fn test_find_missing_data_indexes_sanity() {
        let slot = DEFAULT_SLOT_HEIGHT;

        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes_sanity");
        let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Early exit conditions
        let empty: Vec<u64> = vec![];
        assert_eq!(db_ledger.find_missing_data_indexes(slot, 0, 0, 1), empty);
        assert_eq!(db_ledger.find_missing_data_indexes(slot, 5, 5, 1), empty);
        assert_eq!(db_ledger.find_missing_data_indexes(slot, 4, 3, 1), empty);
        assert_eq!(db_ledger.find_missing_data_indexes(slot, 1, 2, 0), empty);

        let mut blobs = make_tiny_test_entries(2).to_blobs();

        const ONE: u64 = 1;
        const OTHER: u64 = 4;

        blobs[0].set_index(ONE).unwrap();
        blobs[1].set_index(OTHER).unwrap();

        // Insert one blob at index = first_index
        db_ledger.write_blobs(&blobs).unwrap();

        const STARTS: u64 = OTHER * 2;
        const END: u64 = OTHER * 3;
        const MAX: usize = 10;
        // The first blob has index = first_index. Thus, for i < first_index,
        // given the input range of [i, first_index], the missing indexes should be
        // [i, first_index - 1]
        for start in 0..STARTS {
            let result = db_ledger.find_missing_data_indexes(
                slot, start, // start
                END,   //end
                MAX,   //max
            );
            let expected: Vec<u64> = (start..END).filter(|i| *i != ONE && *i != OTHER).collect();
            assert_eq!(result, expected);
        }

        drop(db_ledger);
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_find_missing_data_indexes() {
        let slot = DEFAULT_SLOT_HEIGHT;
        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes");
        let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Write entries
        let gap = 10;
        assert!(gap > 3);
        let num_entries = 10;
        let mut blobs = make_tiny_test_entries(num_entries).to_blobs();
        for (i, b) in blobs.iter_mut().enumerate() {
            b.set_index(i as u64 * gap).unwrap();
            b.set_slot(slot).unwrap();
        }
        db_ledger.write_blobs(&blobs).unwrap();

        // Index of the first blob is 0
        // Index of the second blob is "gap"
        // Thus, the missing indexes should then be [1, gap - 1] for the input index
        // range of [0, gap)
        let expected: Vec<u64> = (1..gap).collect();
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 0, gap, gap as usize),
            expected
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 1, gap, (gap - 1) as usize),
            expected,
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 0, gap - 1, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, gap - 2, gap, gap as usize),
            vec![gap - 2, gap - 1],
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, gap - 2, gap, 1),
            vec![gap - 2],
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 0, gap, 1),
            vec![1],
        );

        // Test with end indexes that are greater than the last item in the ledger
        let mut expected: Vec<u64> = (1..gap).collect();
        expected.push(gap + 1);
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 0, gap + 2, (gap + 2) as usize),
            expected,
        );
        assert_eq!(
            db_ledger.find_missing_data_indexes(slot, 0, gap + 2, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );

        for i in 0..num_entries as u64 {
            for j in 0..i {
                let expected: Vec<u64> = (j..i)
                    .flat_map(|k| {
                        let begin = k * gap + 1;
                        let end = (k + 1) * gap;
                        (begin..end)
                    })
                    .collect();
                assert_eq!(
                    db_ledger.find_missing_data_indexes(
                        slot,
                        j * gap,
                        i * gap,
                        ((i - j) * gap) as usize
                    ),
                    expected,
                );
            }
        }

        drop(db_ledger);
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_no_missing_blob_indexes() {
        let slot = DEFAULT_SLOT_HEIGHT;
        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes");
        let db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Write entries
        let num_entries = 10;
        let shared_blobs = make_tiny_test_entries(num_entries).to_shared_blobs();

        index_blobs(
            shared_blobs.iter().zip(vec![slot; num_entries].into_iter()),
            &Keypair::new().pubkey(),
            0,
        );

        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();
        db_ledger.write_blobs(&blobs).unwrap();

        let empty: Vec<u64> = vec![];
        for i in 0..num_entries as u64 {
            for j in 0..i {
                assert_eq!(
                    db_ledger.find_missing_data_indexes(slot, j, i, (i - j) as usize),
                    empty
                );
            }
        }

        drop(db_ledger);
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }

    #[cfg(all(feature = "erasure", test))]
    #[test]
    pub fn test_try_erasure() {
        // Setup the window
        let offset = 0;
        let num_blobs = NUM_DATA + 2;
        let slot_height = DEFAULT_SLOT_HEIGHT;
        let mut window = setup_window_ledger(offset, num_blobs, false, slot_height);
        let end_index = (offset + num_blobs) % window.len();

        // Test erasing a data block and an erasure block
        let coding_start = offset - (offset % NUM_DATA) + (NUM_DATA - NUM_CODING);

        let erased_index = coding_start % window.len();

        // Create a hole in the window
        let erased_data = window[erased_index].data.clone();
        let erased_coding = window[erased_index].coding.clone().unwrap();
        window[erased_index].data = None;
        window[erased_index].coding = None;

        // Generate the db_ledger from the window
        let ledger_path = get_tmp_ledger_path("test_try_erasure");
        let db_ledger = Arc::new(generate_db_ledger_from_window(&ledger_path, &window, false));

        let mut consume_queue = vec![];
        try_erasure(&db_ledger, &mut consume_queue).expect("Expected successful erasure attempt");
        window[erased_index].data = erased_data;

        {
            let data_blobs: Vec<_> = window[erased_index..end_index]
                .iter()
                .map(|slot| slot.data.clone().unwrap())
                .collect();

            let locks: Vec<_> = data_blobs.iter().map(|blob| blob.read().unwrap()).collect();

            let locked_data: Vec<&Blob> = locks.iter().map(|lock| &**lock).collect();

            let (expected, _) = reconstruct_entries_from_blobs(locked_data).unwrap();
            assert_eq!(consume_queue, expected);
        }

        let erased_coding_l = erased_coding.read().unwrap();
        assert_eq!(
            &db_ledger
                .get_coding_blob_bytes(slot_height, erased_index as u64)
                .unwrap()
                .unwrap()[BLOB_HEADER_SIZE..],
            &erased_coding_l.data()[..erased_coding_l.size().unwrap() as usize],
        );
    }

    #[test]
    fn test_process_blob() {
        // Create the leader scheduler
        let leader_keypair = Keypair::new();
        let mut leader_scheduler = LeaderScheduler::from_bootstrap_leader(leader_keypair.pubkey());

        let db_ledger_path = get_tmp_ledger_path("test_process_blob");
        let db_ledger = Arc::new(DbLedger::open(&db_ledger_path).unwrap());

        // Mock the tick height to look like the tick height right after a leader transition
        leader_scheduler.last_seed_height = None;
        leader_scheduler.use_only_bootstrap_leader = false;

        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));
        let num_entries = 10;
        let original_entries = make_tiny_test_entries(num_entries);
        let shared_blobs = original_entries.clone().to_shared_blobs();

        index_blobs(
            shared_blobs
                .iter()
                .zip(vec![DEFAULT_SLOT_HEIGHT; num_entries].into_iter()),
            &Keypair::new().pubkey(),
            0,
        );

        let mut consume_queue = vec![];
        let mut tick_height = 2;
        let done = Arc::new(AtomicBool::new(false));

        for blob in shared_blobs.iter().rev() {
            process_blob(
                &leader_scheduler,
                &db_ledger,
                blob,
                0,
                &mut consume_queue,
                &mut tick_height,
                &done,
            )
            .expect("Expect successful processing of blob");
        }

        assert_eq!(consume_queue, original_entries);

        drop(db_ledger);
        DbLedger::destroy(&db_ledger_path).expect("Expected successful database destruction");
    }
}
