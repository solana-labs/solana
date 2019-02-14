//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing blobs in the ledger and sends repair requests for those blobs

use crate::blocktree::{Blocktree, SlotMeta};
use crate::cluster_info::ClusterInfo;
use crate::result::Result;
use crate::service::Service;
use solana_metrics::{influxdb, submit};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub const MAX_REPAIR_LENGTH: usize = 16;
pub const REPAIR_MS: u64 = 100;
pub const MAX_REPAIR_TRIES: u64 = 128;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairType {
    HighestBlob(u64, u64),
    Blob(u64, u64),
}

#[derive(Default)]
struct RepairInfo {
    max_slot: u64,
    repair_tries: u64,
}

impl RepairInfo {
    fn new() -> Self {
        RepairInfo {
            max_slot: 0,
            repair_tries: 0,
        }
    }
}

pub struct RepairService {
    t_repair: JoinHandle<()>,
}

impl RepairService {
    fn run(
        blocktree: &Arc<Blocktree>,
        exit: &Arc<AtomicBool>,
        repair_socket: &Arc<UdpSocket>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) {
        let mut repair_info = RepairInfo::new();
        let id = cluster_info.read().unwrap().id();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let repairs = Self::generate_repairs(blocktree, MAX_REPAIR_LENGTH, &mut repair_info);

            if let Ok(repairs) = repairs {
                let reqs: Vec<_> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        let (slot_height, blob_index, is_highest_request) = {
                            match repair_request {
                                RepairType::Blob(s, i) => (s, i, false),
                                RepairType::HighestBlob(s, i) => (s, i, true),
                            }
                        };
                        cluster_info
                            .read()
                            .unwrap()
                            .window_index_request(slot_height, blob_index, is_highest_request)
                            .map(|result| (result, slot_height, blob_index))
                            .ok()
                    })
                    .collect();

                for ((to, req), slot_height, blob_index) in reqs {
                    if let Ok(local_addr) = repair_socket.local_addr() {
                        submit(
                            influxdb::Point::new("repair_service")
                                .add_field(
                                    "repair_slot",
                                    influxdb::Value::Integer(slot_height as i64),
                                )
                                .to_owned()
                                .add_field(
                                    "repair_blob",
                                    influxdb::Value::Integer(blob_index as i64),
                                )
                                .to_owned()
                                .add_field("to", influxdb::Value::String(to.to_string()))
                                .to_owned()
                                .add_field("from", influxdb::Value::String(local_addr.to_string()))
                                .to_owned()
                                .add_field("id", influxdb::Value::String(id.to_string()))
                                .to_owned(),
                        );
                    }

                    repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                        info!("{} repair req send_to({}) error {:?}", id, to, e);
                        0
                    });
                }
            }
            sleep(Duration::from_millis(REPAIR_MS));
        }
    }

    pub fn new(
        blocktree: Arc<Blocktree>,
        exit: Arc<AtomicBool>,
        repair_socket: Arc<UdpSocket>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
    ) -> Self {
        let t_repair = Builder::new()
            .name("solana-repair-service".to_string())
            .spawn(move || Self::run(&blocktree, &exit, &repair_socket, &cluster_info))
            .unwrap();

        RepairService { t_repair }
    }

    fn process_slot(
        blocktree: &Blocktree,
        slot_height: u64,
        slot: &SlotMeta,
        max_repairs: usize,
    ) -> Vec<RepairType> {
        if slot.is_full() {
            vec![]
        } else if slot.consumed == slot.received {
            vec![RepairType::HighestBlob(slot_height, slot.received)]
        } else {
            let reqs = blocktree.find_missing_data_indexes(
                slot_height,
                slot.consumed,
                slot.received,
                max_repairs,
            );

            reqs.into_iter()
                .map(|i| RepairType::Blob(slot_height, i))
                .collect()
        }
    }

    fn generate_repairs(
        blocktree: &Blocktree,
        max_repairs: usize,
        repair_info: &mut RepairInfo,
    ) -> Result<(Vec<RepairType>)> {
        // Slot height and blob indexes for blobs we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        let mut current_slot_height = Some(0);
        while repairs.len() < max_repairs && current_slot_height.is_some() {
            if current_slot_height.unwrap() > repair_info.max_slot {
                repair_info.repair_tries = 0;
                repair_info.max_slot = current_slot_height.unwrap();
            }

            let slot = blocktree.meta(current_slot_height.unwrap())?;
            if slot.is_some() {
                let slot = slot.unwrap();
                let new_repairs = Self::process_slot(
                    blocktree,
                    current_slot_height.unwrap(),
                    &slot,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
            }
            current_slot_height = blocktree.get_next_slot(current_slot_height.unwrap())?;
        }

        // Only increment repair_tries if the ledger contains every blob for every slot
        if repairs.is_empty() {
            repair_info.repair_tries += 1;
        }

        // Optimistically try the next slot if we haven't gotten any repairs
        // for a while
        if repair_info.repair_tries >= MAX_REPAIR_TRIES {
            repairs.push(RepairType::HighestBlob(repair_info.max_slot + 1, 0))
        }

        Ok(repairs)
    }
}

impl Service for RepairService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_repair.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree, BlocktreeConfig};
    use crate::entry::create_ticks;
    use crate::entry::{make_tiny_test_entries, EntrySlice};
    use solana_sdk::hash::Hash;

    #[test]
    #[ignore]
    pub fn test_repair_missed_future_slot() {
        let blocktree_path = get_tmp_ledger_path("test_repair_missed_future_slot");
        {
            let num_ticks_per_slot = 1;
            let blocktree_config = BlocktreeConfig::new(num_ticks_per_slot);
            let blocktree = Blocktree::open_config(&blocktree_path, &blocktree_config).unwrap();

            let mut blobs = create_ticks(1, Hash::default()).to_blobs();
            blobs[0].set_index(0);
            blobs[0].set_slot(0);

            blocktree.write_blobs(&blobs).unwrap();

            let mut repair_info = RepairInfo::new();
            // We have all the blobs for all the slots in the ledger, wait for optimistic
            // future repair after MAX_REPAIR_TRIES
            for i in 0..MAX_REPAIR_TRIES {
                // Check that repair tries to patch the empty slot
                assert_eq!(repair_info.repair_tries, i);
                assert_eq!(repair_info.max_slot, 0);
                let expected = if i == MAX_REPAIR_TRIES - 1 {
                    vec![RepairType::Blob(1, 0)]
                } else {
                    vec![]
                };
                assert_eq!(
                    RepairService::generate_repairs(&blocktree, 2, &mut repair_info).unwrap(),
                    expected
                );
            }

            // Insert a bigger blob
            let mut blobs = create_ticks(1, Hash::default()).to_blobs();
            blobs[0].set_index(0);
            blobs[0].set_slot(1);

            blocktree.write_blobs(&blobs).unwrap();
            assert_eq!(
                RepairService::generate_repairs(&blocktree, 2, &mut repair_info).unwrap(),
                vec![]
            );
            assert_eq!(repair_info.repair_tries, 1);
            assert_eq!(repair_info.max_slot, 1);
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    #[ignore]
    pub fn test_repair_empty_slot() {
        let blocktree_path = get_tmp_ledger_path("test_repair_empty_slot");
        {
            let num_ticks_per_slot = 10;
            let blocktree_config = BlocktreeConfig::new(num_ticks_per_slot);
            let blocktree = Blocktree::open_config(&blocktree_path, &blocktree_config).unwrap();

            let mut blobs = make_tiny_test_entries(1).to_blobs();
            blobs[0].set_index(1);
            blobs[0].set_slot(2);

            let mut repair_info = RepairInfo::new();

            // Write this blob to slot 2, should chain to slot 1, which we haven't received
            // any blobs for
            blocktree.write_blobs(&blobs).unwrap();
            // Check that repair tries to patch the empty slot
            assert_eq!(
                RepairService::generate_repairs(&blocktree, 2, &mut repair_info).unwrap(),
                vec![RepairType::Blob(1, 0), RepairType::Blob(2, 0)]
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    #[ignore]
    pub fn test_generate_repairs() {
        let blocktree_path = get_tmp_ledger_path("test_generate_repairs");
        {
            let num_ticks_per_slot = 10;
            let blocktree_config = BlocktreeConfig::new(num_ticks_per_slot);
            let blocktree = Blocktree::open_config(&blocktree_path, &blocktree_config).unwrap();

            let num_entries_per_slot = 10;
            let num_slots = 2;
            let mut blobs = make_tiny_test_entries(num_slots * num_entries_per_slot).to_blobs();

            let mut repair_info = RepairInfo::new();

            // Insert every nth entry for each slot
            let nth = 3;
            for (i, b) in blobs.iter_mut().enumerate() {
                b.set_index(((i % num_entries_per_slot) * nth) as u64);
                b.set_slot((i / num_entries_per_slot) as u64);
            }

            blocktree.write_blobs(&blobs).unwrap();

            let missing_indexes_per_slot: Vec<u64> = (0..num_entries_per_slot - 1)
                .flat_map(|x| ((nth * x + 1) as u64..(nth * x + nth) as u64))
                .collect();

            let expected: Vec<RepairType> = (0..num_slots)
                .flat_map(|slot_height| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |blob_index| RepairType::Blob(slot_height as u64, *blob_index))
                })
                .collect();

            // Across all slots, find all missing indexes in the range [0, num_entries_per_slot * nth]
            assert_eq!(
                RepairService::generate_repairs(&blocktree, std::usize::MAX, &mut repair_info)
                    .unwrap(),
                expected
            );

            assert_eq!(
                RepairService::generate_repairs(&blocktree, expected.len() - 2, &mut repair_info)
                    .unwrap()[..],
                expected[0..expected.len() - 2]
            );

            // Now fill in all the holes for each slot such that for each slot, consumed == received.
            // Because none of the slots contain ticks, we should see that the repair requests
            // ask for ticks, starting from the last received index for that slot
            for repair in expected {
                match repair {
                    RepairType::Blob(slot_height, blob_index) => {
                        let mut b = make_tiny_test_entries(1).to_blobs().pop().unwrap();
                        b.set_index(blob_index);
                        b.set_slot(slot_height);
                        blocktree.write_blobs(&vec![b]).unwrap();
                    }

                    _ => panic!("Unexpected repair type"),
                }
            }

            let last_index_per_slot = ((num_entries_per_slot - 1) * nth) as u64;
            let missing_indexes_per_slot: Vec<u64> =
                (last_index_per_slot + 1..last_index_per_slot + 1 + num_ticks_per_slot).collect();
            let expected: Vec<RepairType> = (0..num_slots)
                .flat_map(|slot_height| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |blob_index| RepairType::Blob(slot_height as u64, *blob_index))
                })
                .collect();
            assert_eq!(
                RepairService::generate_repairs(&blocktree, std::usize::MAX, &mut repair_info)
                    .unwrap(),
                expected
            );
            assert_eq!(
                RepairService::generate_repairs(&blocktree, expected.len() - 2, &mut repair_info)
                    .unwrap()[..],
                expected[0..expected.len() - 2]
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
}
