//! The `repair_service` module implements the tools necessary to generate a thread which
//! regularly finds missing blobs in the ledger and sends repair requests for those blobs

use crate::bank_forks::BankForks;
use crate::blocktree::{Blocktree, SlotMeta};
use crate::cluster_info::ClusterInfo;
use crate::result::Result;
use crate::service::Service;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub const MAX_REPAIR_LENGTH: usize = 16;
pub const REPAIR_MS: u64 = 100;
pub const MAX_REPAIR_TRIES: u64 = 128;
pub const NUM_FORKS_TO_REPAIR: usize = 5;
pub const MAX_ORPHANS: usize = 5;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairType {
    Orphan(u64),
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

pub struct RepairSlotRange {
    pub start: u64,
    pub end: u64,
}

impl Default for RepairSlotRange {
    fn default() -> Self {
        RepairSlotRange {
            start: 0,
            end: std::u64::MAX,
        }
    }
}

pub struct RepairService {
    t_repair: JoinHandle<()>,
}

impl RepairService {
    pub fn new(
        blocktree: Arc<Blocktree>,
        exit: &Arc<AtomicBool>,
        repair_socket: Arc<UdpSocket>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        repair_slot_range: Option<RepairSlotRange>,
    ) -> Self {
        let exit = exit.clone();
        let t_repair = Builder::new()
            .name("solana-repair-service".to_string())
            .spawn(move || {
                Self::run(
                    &blocktree,
                    exit,
                    &repair_socket,
                    &cluster_info,
                    &bank_forks,
                    repair_slot_range,
                )
            })
            .unwrap();

        RepairService { t_repair }
    }

    fn run(
        blocktree: &Arc<Blocktree>,
        exit: Arc<AtomicBool>,
        repair_socket: &Arc<UdpSocket>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        bank_forks: &Option<Arc<RwLock<BankForks>>>,
        repair_slot_range: Option<RepairSlotRange>,
    ) {
        let mut repair_info = RepairInfo::new();
        let epoch_slots: HashSet<u64> = HashSet::new();
        let id = cluster_info.read().unwrap().id();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let repairs = {
                if let Some(ref repair_slot_range) = repair_slot_range {
                    // Strategy used by replicators
                    Self::generate_repairs_in_range(
                        blocktree,
                        MAX_REPAIR_LENGTH,
                        &mut repair_info,
                        repair_slot_range,
                    )
                } else {
                    let bank_forks = bank_forks
                        .as_ref()
                        .expect("Non-replicator repair strategy missing BankForks");
                    Self::update_fast_repair(id, &epoch_slots, &cluster_info, bank_forks);
                    Self::generate_repairs(blocktree, MAX_REPAIR_LENGTH)
                }
            };

            if let Ok(repairs) = repairs {
                let reqs: Vec<_> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        cluster_info
                            .read()
                            .unwrap()
                            .repair_request(&repair_request)
                            .map(|result| (result, repair_request))
                            .ok()
                    })
                    .collect();

                for ((to, req), repair_request) in reqs {
                    if let Ok(local_addr) = repair_socket.local_addr() {
                        submit(
                            influxdb::Point::new("repair_service")
                                .add_field(
                                    "repair_request",
                                    influxdb::Value::String(format!("{:?}", repair_request)),
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

    fn generate_repairs_in_range(
        blocktree: &Blocktree,
        max_repairs: usize,
        repair_info: &mut RepairInfo,
        repair_range: &RepairSlotRange,
    ) -> Result<(Vec<RepairType>)> {
        // Slot height and blob indexes for blobs we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        let mut current_slot = Some(repair_range.start);
        while repairs.len() < max_repairs && current_slot.is_some() {
            if current_slot.unwrap() > repair_range.end {
                break;
            }

            if current_slot.unwrap() > repair_info.max_slot {
                repair_info.repair_tries = 0;
                repair_info.max_slot = current_slot.unwrap();
            }

            if let Some(slot) = blocktree.meta(current_slot.unwrap())? {
                let new_repairs = Self::generate_repairs_for_slot(
                    blocktree,
                    current_slot.unwrap(),
                    &slot,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
            }
            current_slot = blocktree.get_next_slot(current_slot.unwrap())?;
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

    fn generate_repairs(blocktree: &Blocktree, max_repairs: usize) -> Result<(Vec<RepairType>)> {
        // Slot height and blob indexes for blobs we want to repair
        let mut repairs: Vec<RepairType> = vec![];
        let slot = blocktree.get_root()?;
        Self::generate_repairs_for_fork(blocktree, &mut repairs, max_repairs, slot);

        // TODO: Incorporate gossip to determine priorities for repair?

        // Try to resolve orphans in blocktree
        let orphans = blocktree.get_orphans(Some(MAX_ORPHANS));

        Self::generate_repairs_for_orphans(&orphans[..], &mut repairs);
        Ok(repairs)
    }

    fn generate_repairs_for_slot(
        blocktree: &Blocktree,
        slot: u64,
        slot_meta: &SlotMeta,
        max_repairs: usize,
    ) -> Vec<RepairType> {
        if slot_meta.is_full() {
            vec![]
        } else if slot_meta.consumed == slot_meta.received {
            vec![RepairType::HighestBlob(slot, slot_meta.received)]
        } else {
            let reqs = blocktree.find_missing_data_indexes(
                slot,
                slot_meta.consumed,
                slot_meta.received,
                max_repairs,
            );

            reqs.into_iter()
                .map(|i| RepairType::Blob(slot, i))
                .collect()
        }
    }

    fn generate_repairs_for_orphans(orphans: &[u64], repairs: &mut Vec<RepairType>) {
        repairs.extend(orphans.iter().map(|h| RepairType::Orphan(*h)));
    }

    /// Repairs any fork starting at the input slot
    fn generate_repairs_for_fork(
        blocktree: &Blocktree,
        repairs: &mut Vec<RepairType>,
        max_repairs: usize,
        slot: u64,
    ) {
        let mut pending_slots = vec![slot];
        while repairs.len() < max_repairs && !pending_slots.is_empty() {
            let slot = pending_slots.pop().unwrap();
            if let Some(slot_meta) = blocktree.meta(slot).unwrap() {
                let new_repairs = Self::generate_repairs_for_slot(
                    blocktree,
                    slot,
                    &slot_meta,
                    max_repairs - repairs.len(),
                );
                repairs.extend(new_repairs);
                let next_slots = slot_meta.next_slots;
                pending_slots.extend(next_slots);
            } else {
                break;
            }
        }
    }

    fn update_fast_repair(
        id: Pubkey,
        slots: &HashSet<u64>,
        cluster_info: &RwLock<ClusterInfo>,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let root = bank_forks.read().unwrap().root();
        cluster_info
            .write()
            .unwrap()
            .push_epoch_slots(id, root, slots.clone());
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
    use crate::blocktree::tests::{make_many_slot_entries, make_slot_entries};
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};

    #[test]
    pub fn test_repair_orphan() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            // Create some orphan slots
            let (mut blobs, _) = make_slot_entries(1, 0, 1);
            let (blobs2, _) = make_slot_entries(5, 2, 1);
            blobs.extend(blobs2);
            blocktree.write_blobs(&blobs).unwrap();
            assert_eq!(
                RepairService::generate_repairs(&blocktree, 2).unwrap(),
                vec![
                    RepairType::HighestBlob(0, 0),
                    RepairType::Orphan(0),
                    RepairType::Orphan(2)
                ]
            );
        }

        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_empty_slot() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            let (blobs, _) = make_slot_entries(2, 0, 1);

            // Write this blob to slot 2, should chain to slot 0, which we haven't received
            // any blobs for
            blocktree.write_blobs(&blobs).unwrap();

            // Check that repair tries to patch the empty slot
            assert_eq!(
                RepairService::generate_repairs(&blocktree, 2).unwrap(),
                vec![RepairType::HighestBlob(0, 0), RepairType::Orphan(0)]
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_repairs() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            let nth = 3;
            let num_entries_per_slot = 5 * nth;
            let num_slots = 2;

            // Create some blobs
            let (blobs, _) =
                make_many_slot_entries(0, num_slots as u64, num_entries_per_slot as u64);

            // write every nth blob
            let blobs_to_write: Vec<_> = blobs.iter().step_by(nth as usize).collect();

            blocktree.write_blobs(blobs_to_write).unwrap();

            let missing_indexes_per_slot: Vec<u64> = (0..num_entries_per_slot / nth - 1)
                .flat_map(|x| ((nth * x + 1) as u64..(nth * x + nth) as u64))
                .collect();

            let expected: Vec<RepairType> = (0..num_slots)
                .flat_map(|slot| {
                    missing_indexes_per_slot
                        .iter()
                        .map(move |blob_index| RepairType::Blob(slot as u64, *blob_index))
                })
                .collect();

            assert_eq!(
                RepairService::generate_repairs(&blocktree, std::usize::MAX).unwrap(),
                expected
            );

            assert_eq!(
                RepairService::generate_repairs(&blocktree, expected.len() - 2).unwrap()[..],
                expected[0..expected.len() - 2]
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_generate_highest_repair() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            let num_entries_per_slot = 10;

            // Create some blobs
            let (mut blobs, _) = make_slot_entries(0, 0, num_entries_per_slot as u64);

            // Remove is_last flag on last blob
            blobs.last_mut().unwrap().set_flags(0);

            blocktree.write_blobs(&blobs).unwrap();

            // We didn't get the last blob for this slot, so ask for the highest blob for that slot
            let expected: Vec<RepairType> = vec![RepairType::HighestBlob(0, num_entries_per_slot)];

            assert_eq!(
                RepairService::generate_repairs(&blocktree, std::usize::MAX).unwrap(),
                expected
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }

    #[test]
    pub fn test_repair_range() {
        let blocktree_path = get_tmp_ledger_path!();
        {
            let blocktree = Blocktree::open(&blocktree_path).unwrap();

            let num_entries_per_slot = 10;

            let mut repair_info = RepairInfo::new();

            let num_slots = 1;
            let start = 5;
            // Create some blobs in slots 0..num_slots
            for i in start..start + num_slots {
                let parent = if i > 0 { i - 1 } else { 0 };
                let (blobs, _) = make_slot_entries(i, parent, num_entries_per_slot as u64);

                blocktree.write_blobs(&blobs).unwrap();
            }

            let end = 4;
            let expected: Vec<RepairType> = vec![RepairType::HighestBlob(end, 0)];

            let mut repair_slot_range = RepairSlotRange::default();
            repair_slot_range.start = 2;
            repair_slot_range.end = end;

            assert_eq!(
                RepairService::generate_repairs_in_range(
                    &blocktree,
                    std::usize::MAX,
                    &mut repair_info,
                    &repair_slot_range
                )
                .unwrap(),
                expected
            );
        }
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
    }
}
