//! Set of functions for emulating windowing functions from a database ledger implementation
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::db_ledger::*;
use crate::entry::Entry;
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::{SharedBlob, BLOB_HEADER_SIZE};
use crate::result::Result;
use crate::streamer::BlobSender;
#[cfg(feature = "erasure")]
use crate::erasure;
use log::Level;
use rocksdb::DBRawIterator;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use std::cmp;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

pub const MAX_REPAIR_LENGTH: usize = 128;

pub fn repair(
    slot: u64,
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
    let meta = db_ledger.meta_cf.get(&db_ledger.db, &MetaCf::key(slot))?;
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
                    // 1) The replicate stage hasn't caught up to the "consumed" entries we sent,
                    // in which case it will eventually catch up
                    //
                    // 2) We are on the border between seed_rotation_intervals, so the
                    // schedule won't be known until the entry on that cusp is received
                    // by the replicate stage (which comes after this stage). Hence, the next
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

    let num_peers = rcluster_info.tvu_peers().len() as u64;

    // Check if there's a max_entry_height limitation
    let max_repair_entry_height = if max_entry_height == 0 {
        calculate_max_repair_entry_height(num_peers, consumed, received, times, is_next_leader)
    } else {
        max_entry_height + 2
    };

    let idxs = find_missing_data_indexes(
        slot,
        db_ledger,
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

// Given a start and end entry index, find all the missing
// indexes in the ledger in the range [start_index, end_index)
pub fn find_missing_indexes(
    db_iterator: &mut DBRawIterator,
    slot: u64,
    start_index: u64,
    end_index: u64,
    key: &Fn(u64, u64) -> Vec<u8>,
    index_from_key: &Fn(&[u8]) -> Result<u64>,
    max_missing: usize,
) -> Vec<u64> {
    if start_index >= end_index || max_missing == 0 {
        return vec![];
    }

    let mut missing_indexes = vec![];

    // Seek to the first blob with index >= start_index
    db_iterator.seek(&key(slot, start_index));

    // The index of the first missing blob in the slot
    let mut prev_index = start_index;
    'outer: loop {
        if !db_iterator.valid() {
            for i in prev_index..end_index {
                missing_indexes.push(i);
                if missing_indexes.len() == max_missing {
                    break;
                }
            }
            break;
        }
        let current_key = db_iterator.key().expect("Expect a valid key");
        let current_index =
            index_from_key(&current_key).expect("Expect to be able to parse index from valid key");
        let upper_index = cmp::min(current_index, end_index);
        for i in prev_index..upper_index {
            missing_indexes.push(i);
            if missing_indexes.len() == max_missing {
                break 'outer;
            }
        }
        if current_index >= end_index {
            break;
        }

        prev_index = current_index + 1;
        db_iterator.next();
    }

    missing_indexes
}

pub fn find_missing_data_indexes(
    slot: u64,
    db_ledger: &DbLedger,
    start_index: u64,
    end_index: u64,
    max_missing: usize,
) -> Vec<u64> {
    let mut db_iterator = db_ledger
        .db
        .raw_iterator_cf(db_ledger.data_cf.handle(&db_ledger.db))
        .expect("Expected to be able to open database iterator");

    find_missing_indexes(
        &mut db_iterator,
        slot,
        start_index,
        end_index,
        &DataCf::key,
        &DataCf::index_from_key,
        max_missing,
    )
}

pub fn find_missing_coding_indexes(
    slot: u64,
    db_ledger: &DbLedger,
    start_index: u64,
    end_index: u64,
    max_missing: usize,
) -> Vec<u64> {
    let mut db_iterator = db_ledger
        .db
        .raw_iterator_cf(db_ledger.erasure_cf.handle(&db_ledger.db))
        .expect("Expected to be able to open database iterator");

    find_missing_indexes(
        &mut db_iterator,
        slot,
        start_index,
        end_index,
        &ErasureCf::key,
        &ErasureCf::index_from_key,
        max_missing,
    )
}

pub fn retransmit_all_leader_blocks(
    dq: &[SharedBlob],
    leader_scheduler: &LeaderScheduler,
    retransmit: &BlobSender,
) -> Result<()> {
    let mut retransmit_queue: Vec<SharedBlob> = Vec::new();
    for b in dq {
        // Check if the blob is from the scheduled leader for its slot. If so,
        // add to the retransmit_queue
        if let Ok(slot) = b.read().unwrap().slot() {
            if let Some(leader_id) = leader_scheduler.get_leader_for_slot(slot) {
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
    leader_scheduler: &LeaderScheduler,
    db_ledger: &mut DbLedger,
    blob: &SharedBlob,
    max_ix: u64,
    pix: u64,
    consume_queue: &mut Vec<Entry>,
    tick_height: &mut u64,
    done: &Arc<AtomicBool>,
) -> Result<()> {
    let is_coding = blob.read().unwrap().is_coding();

    // Check if the blob is in the range of our known leaders. If not, we return.
    // TODO: Need to update slot in broadcast, otherwise this check will fail with
    // leader rotation enabled
    // Github issue: https://github.com/solana-labs/solana/issues/1899.
    let slot = blob.read().unwrap().slot()?;
    let leader = leader_scheduler.get_leader_for_slot(slot);

    // TODO: Once the original leader signature is added to the blob, make sure that
    // the blob was originally generated by the expected leader for this slot

    if leader.is_none() {
        return Ok(());
    }

    // Insert the new blob into the window
    let mut consumed_entries = if is_coding {
        let erasure_key = ErasureCf::key(slot, pix);
        let rblob = &blob.read().unwrap();
        let size = rblob.size()?;
        db_ledger.erasure_cf.put(
            &db_ledger.db,
            &erasure_key,
            &rblob.data[..BLOB_HEADER_SIZE + size],
        )?;
        vec![]
    } else {
        let data_key = DataCf::key(slot, pix);
        db_ledger.insert_data_blob(&data_key, &blob.read().unwrap())?
    };

    #[cfg(feature = "erasure")]
    {
        // If write_shared_blobs() of these recovered blobs fails fails, don't return
        // because consumed_entries might be nonempty from earlier, and tick height needs to
        // be updated. Hopefully we can recover these blobs next time successfully.
        if let Err(e) = try_erasure(db_ledger, slot, consume_queue) {
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
            .meta_cf
            .get(&db_ledger.db, &MetaCf::key(slot))?
            .expect("Expect metadata to exist if consumed entries is nonzero");

        let consumed = meta.consumed;

        // Check if we ran over the last wanted entry
        if consumed > max_ix {
            let extra_unwanted_entries_len = consumed - (max_ix + 1);
            let consumed_entries_len = consumed_entries.len();
            consumed_entries.truncate(consumed_entries_len - extra_unwanted_entries_len as usize);
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
fn try_erasure(db_ledger: &mut DbLedger, slot: u64, consume_queue: &mut Vec<Entry>) -> Result<()> {
    let meta = db_ledger.meta_cf.get(&db_ledger.db, &MetaCf::key(slot))?;
    if let Some(meta) = meta {
        let (data, coding) = erasure::recover(db_ledger, slot, meta.consumed)?;
        for c in coding {
            let cl = c.read().unwrap();
            let erasure_key =
                ErasureCf::key(slot, cl.index().expect("Recovered blob must set index"));
            let size = cl.size().expect("Recovered blob must set size");
            db_ledger.erasure_cf.put(
                &db_ledger.db,
                &erasure_key,
                &cl.data[..BLOB_HEADER_SIZE + size],
            )?;
        }

        let entries = db_ledger.write_shared_blobs(slot, data)?;
        consume_queue.extend(entries);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::ledger::{get_tmp_ledger_path, make_tiny_test_entries, Block};
    use crate::packet::{Blob, Packet, Packets, SharedBlob, PACKET_DATA_SIZE};
    use crate::streamer::{receiver, responder, PacketReceiver};
    #[cfg(all(feature = "erasure", test))]
    use crate::entry::reconstruct_entries_from_blobs;
    #[cfg(all(feature = "erasure", test))]
    use crate::erasure::test::{generate_db_ledger_from_window, setup_window_ledger};
    #[cfg(all(feature = "erasure", test))]
    use crate::erasure::{NUM_CODING, NUM_DATA};
    use rocksdb::{Options, DB};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
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
                let mut b = SharedBlob::default();
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
        let leader_scheduler = LeaderScheduler::from_bootstrap_leader(leader);
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
        let slot = 0;

        // Create RocksDb ledger
        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes_sanity");
        let mut db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Early exit conditions
        let empty: Vec<u64> = vec![];
        assert_eq!(find_missing_data_indexes(slot, &db_ledger, 0, 0, 1), empty);
        assert_eq!(find_missing_data_indexes(slot, &db_ledger, 5, 5, 1), empty);
        assert_eq!(find_missing_data_indexes(slot, &db_ledger, 4, 3, 1), empty);
        assert_eq!(find_missing_data_indexes(slot, &db_ledger, 1, 2, 0), empty);

        let shared_blob = &make_tiny_test_entries(1).to_blobs()[0];
        let first_index = 10;
        {
            let mut bl = shared_blob.write().unwrap();
            bl.set_index(10).unwrap();
        }

        // Insert one blob at index = first_index
        db_ledger
            .write_blobs(slot, &vec![&*shared_blob.read().unwrap()])
            .unwrap();

        // The first blob has index = first_index. Thus, for i < first_index,
        // given the input range of [i, first_index], the missing indexes should be
        // [i, first_index - 1]
        for i in 0..first_index {
            let result = find_missing_data_indexes(
                slot,
                &db_ledger,
                i,
                first_index,
                (first_index - i) as usize,
            );
            let expected: Vec<u64> = (i..first_index).collect();

            assert_eq!(result, expected);
        }

        drop(db_ledger);
        DB::destroy(&Options::default(), &db_ledger_path)
            .expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_find_missing_data_indexes() {
        let slot = 0;
        // Create RocksDb ledger
        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes");
        let mut db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Write entries
        let gap = 10;
        assert!(gap > 3);
        let num_entries = 10;
        let shared_blobs = make_tiny_test_entries(num_entries).to_blobs();
        for (i, b) in shared_blobs.iter().enumerate() {
            b.write().unwrap().set_index(i as u64 * gap).unwrap();
        }
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();
        db_ledger.write_blobs(slot, &blobs).unwrap();

        // Index of the first blob is 0
        // Index of the second blob is "gap"
        // Thus, the missing indexes should then be [1, gap - 1] for the input index
        // range of [0, gap)
        let expected: Vec<u64> = (1..gap).collect();
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 0, gap, gap as usize),
            expected
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 1, gap, (gap - 1) as usize),
            expected,
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 0, gap - 1, (gap - 1) as usize),
            &expected[..expected.len() - 1],
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, gap - 2, gap, gap as usize),
            vec![gap - 2, gap - 1],
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, gap - 2, gap, 1),
            vec![gap - 2],
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 0, gap, 1),
            vec![1],
        );

        // Test with end indexes that are greater than the last item in the ledger
        let mut expected: Vec<u64> = (1..gap).collect();
        expected.push(gap + 1);
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 0, gap + 2, (gap + 2) as usize),
            expected,
        );
        assert_eq!(
            find_missing_data_indexes(slot, &db_ledger, 0, gap + 2, (gap - 1) as usize),
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
                    find_missing_data_indexes(
                        slot,
                        &db_ledger,
                        j * gap,
                        i * gap,
                        ((i - j) * gap) as usize
                    ),
                    expected,
                );
            }
        }

        drop(db_ledger);
        DB::destroy(&Options::default(), &db_ledger_path)
            .expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_no_missing_blob_indexes() {
        let slot = 0;
        // Create RocksDb ledger
        let db_ledger_path = get_tmp_ledger_path("test_find_missing_data_indexes");
        let mut db_ledger = DbLedger::open(&db_ledger_path).unwrap();

        // Write entries
        let num_entries = 10;
        let shared_blobs = make_tiny_test_entries(num_entries).to_blobs();
        let blob_locks: Vec<_> = shared_blobs.iter().map(|b| b.read().unwrap()).collect();
        let blobs: Vec<&Blob> = blob_locks.iter().map(|b| &**b).collect();
        db_ledger.write_blobs(slot, &blobs).unwrap();

        let empty: Vec<u64> = vec![];
        for i in 0..num_entries as u64 {
            for j in 0..i {
                assert_eq!(
                    find_missing_data_indexes(slot, &db_ledger, j, i, (i - j) as usize),
                    empty
                );
            }
        }

        drop(db_ledger);
        DB::destroy(&Options::default(), &db_ledger_path)
            .expect("Expected successful database destruction");
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

        let erase_offset = coding_start % window.len();

        // Create a hole in the window
        let erased_data = window[erase_offset].data.clone();
        let erased_coding = window[erase_offset].coding.clone().unwrap();
        window[erase_offset].data = None;
        window[erase_offset].coding = None;

        // Generate the db_ledger from the window
        let ledger_path = get_tmp_ledger_path("test_try_erasure");
        let mut db_ledger =
            generate_db_ledger_from_window(&ledger_path, &window, slot_height, false);

        let mut consume_queue = vec![];
        try_erasure(&mut db_ledger, slot_height, &mut consume_queue)
            .expect("Expected successful erasure attempt");
        window[erase_offset].data = erased_data;

        let data_blobs: Vec<_> = window[erase_offset..end_index]
            .iter()
            .map(|slot| slot.data.clone().unwrap())
            .collect();
        let (expected, _) = reconstruct_entries_from_blobs(data_blobs).unwrap();
        assert_eq!(consume_queue, expected);

        let erased_coding_l = erased_coding.read().unwrap();
        assert_eq!(
            &db_ledger
                .erasure_cf
                .get_by_slot_index(&db_ledger.db, slot_height, erase_offset as u64)
                .unwrap()
                .unwrap()[BLOB_HEADER_SIZE..],
            &erased_coding_l.data()[..erased_coding_l.size().unwrap() as usize],
        );
    }
}
