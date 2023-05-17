use {
    crate::ancestor_hashes_service::{
        AncestorHashesReplayUpdate, AncestorHashesReplayUpdateSender,
    },
    solana_consensus::{
        fork_choice::ForkChoice, heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
    },
    solana_ledger::blockstore::Blockstore,
    solana_sdk::{clock::Slot, hash::Hash},
    std::collections::{BTreeMap, BTreeSet, HashMap},
};

pub type DuplicateSlotsTracker = BTreeSet<Slot>;
pub type DuplicateSlotsToRepair = HashMap<Slot, Hash>;
pub type PurgeRepairSlotCounter = BTreeMap<Slot, usize>;
pub type EpochSlotsFrozenSlots = BTreeMap<Slot, Hash>;
pub type GossipDuplicateConfirmedSlots = BTreeMap<Slot, Hash>;

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ClusterConfirmedHash {
    // Ordered from strongest confirmation to weakest. Stronger
    // confirmations take precedence over weaker ones.
    DuplicateConfirmed(Hash),
    EpochSlotsFrozen(Hash),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum BankStatus {
    Frozen(Hash),
    Dead,
    Unprocessed,
}

impl BankStatus {
    pub fn new(is_dead: impl Fn() -> bool, get_hash: impl Fn() -> Option<Hash>) -> Self {
        if is_dead() {
            Self::new_dead()
        } else {
            Self::new_from_hash(get_hash())
        }
    }

    fn new_dead() -> Self {
        BankStatus::Dead
    }

    fn new_from_hash(hash: Option<Hash>) -> Self {
        if let Some(hash) = hash {
            if hash == Hash::default() {
                BankStatus::Unprocessed
            } else {
                BankStatus::Frozen(hash)
            }
        } else {
            BankStatus::Unprocessed
        }
    }

    fn bank_hash(&self) -> Option<Hash> {
        match self {
            BankStatus::Frozen(hash) => Some(*hash),
            BankStatus::Dead => Some(Hash::default()),
            BankStatus::Unprocessed => None,
        }
    }

    fn is_dead(&self) -> bool {
        match self {
            BankStatus::Frozen(_) => false,
            BankStatus::Dead => true,
            BankStatus::Unprocessed => false,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct DeadState {
    // Keep fields private, forces construction
    // via constructor
    cluster_confirmed_hash: Option<ClusterConfirmedHash>,
    is_slot_duplicate: bool,
}

impl DeadState {
    pub fn new_from_state(
        slot: Slot,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
    ) -> Self {
        let cluster_confirmed_hash = get_cluster_confirmed_hash_from_state(
            slot,
            gossip_duplicate_confirmed_slots,
            epoch_slots_frozen_slots,
            fork_choice,
            Some(Hash::default()),
        );
        let is_slot_duplicate = duplicate_slots_tracker.contains(&slot);
        Self::new(cluster_confirmed_hash, is_slot_duplicate)
    }

    // pub for tests
    pub fn new(
        cluster_confirmed_hash: Option<ClusterConfirmedHash>,
        is_slot_duplicate: bool,
    ) -> Self {
        Self {
            cluster_confirmed_hash,
            is_slot_duplicate,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct BankFrozenState {
    // Keep fields private, forces construction
    // via constructor
    frozen_hash: Hash,
    cluster_confirmed_hash: Option<ClusterConfirmedHash>,
    is_slot_duplicate: bool,
}

impl BankFrozenState {
    pub fn new_from_state(
        slot: Slot,
        frozen_hash: Hash,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        epoch_slots_frozen_slots: &EpochSlotsFrozenSlots,
    ) -> Self {
        let cluster_confirmed_hash = get_cluster_confirmed_hash_from_state(
            slot,
            gossip_duplicate_confirmed_slots,
            epoch_slots_frozen_slots,
            fork_choice,
            Some(frozen_hash),
        );
        let is_slot_duplicate = duplicate_slots_tracker.contains(&slot);
        Self::new(frozen_hash, cluster_confirmed_hash, is_slot_duplicate)
    }

    // pub for tests
    pub fn new(
        frozen_hash: Hash,
        cluster_confirmed_hash: Option<ClusterConfirmedHash>,
        is_slot_duplicate: bool,
    ) -> Self {
        assert!(frozen_hash != Hash::default());
        Self {
            frozen_hash,
            cluster_confirmed_hash,
            is_slot_duplicate,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct DuplicateConfirmedState {
    // Keep fields private, forces construction
    // via constructor
    duplicate_confirmed_hash: Hash,
    bank_status: BankStatus,
}
impl DuplicateConfirmedState {
    pub fn new_from_state(
        duplicate_confirmed_hash: Hash,
        is_dead: impl Fn() -> bool,
        get_hash: impl Fn() -> Option<Hash>,
    ) -> Self {
        let bank_status = BankStatus::new(is_dead, get_hash);
        Self::new(duplicate_confirmed_hash, bank_status)
    }

    // pub for tests
    pub fn new(duplicate_confirmed_hash: Hash, bank_status: BankStatus) -> Self {
        Self {
            duplicate_confirmed_hash,
            bank_status,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct DuplicateState {
    // Keep fields private, forces construction
    // via constructor
    duplicate_confirmed_hash: Option<Hash>,
    bank_status: BankStatus,
}
impl DuplicateState {
    pub fn new_from_state(
        slot: Slot,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        is_dead: impl Fn() -> bool,
        get_hash: impl Fn() -> Option<Hash>,
    ) -> Self {
        let bank_status = BankStatus::new(is_dead, get_hash);

        // We can only skip marking duplicate if this slot has already been
        // duplicate confirmed, any weaker confirmation levels are not sufficient
        // to skip marking the slot as duplicate.
        let duplicate_confirmed_hash = get_duplicate_confirmed_hash_from_state(
            slot,
            gossip_duplicate_confirmed_slots,
            fork_choice,
            bank_status.bank_hash(),
        );
        Self::new(duplicate_confirmed_hash, bank_status)
    }

    // pub for tests
    pub fn new(duplicate_confirmed_hash: Option<Hash>, bank_status: BankStatus) -> Self {
        Self {
            duplicate_confirmed_hash,
            bank_status,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct EpochSlotsFrozenState {
    // Keep fields private, forces construction
    // via constructor
    epoch_slots_frozen_hash: Hash,
    duplicate_confirmed_hash: Option<Hash>,
    bank_status: BankStatus,
}
impl EpochSlotsFrozenState {
    pub fn new_from_state(
        slot: Slot,
        epoch_slots_frozen_hash: Hash,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        is_dead: impl Fn() -> bool,
        get_hash: impl Fn() -> Option<Hash>,
    ) -> Self {
        let bank_status = BankStatus::new(is_dead, get_hash);
        let duplicate_confirmed_hash = get_duplicate_confirmed_hash_from_state(
            slot,
            gossip_duplicate_confirmed_slots,
            fork_choice,
            bank_status.bank_hash(),
        );
        Self::new(
            epoch_slots_frozen_hash,
            duplicate_confirmed_hash,
            bank_status,
        )
    }

    // pub for tests
    pub fn new(
        epoch_slots_frozen_hash: Hash,
        duplicate_confirmed_hash: Option<Hash>,
        bank_status: BankStatus,
    ) -> Self {
        Self {
            epoch_slots_frozen_hash,
            duplicate_confirmed_hash,
            bank_status,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum SlotStateUpdate {
    BankFrozen(BankFrozenState),
    DuplicateConfirmed(DuplicateConfirmedState),
    Dead(DeadState),
    Duplicate(DuplicateState),
    EpochSlotsFrozen(EpochSlotsFrozenState),
}

impl SlotStateUpdate {
    fn bank_hash(&self) -> Option<Hash> {
        match self {
            SlotStateUpdate::BankFrozen(bank_frozen_state) => Some(bank_frozen_state.frozen_hash),
            SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state) => {
                duplicate_confirmed_state.bank_status.bank_hash()
            }
            SlotStateUpdate::Dead(_) => Some(Hash::default()),
            SlotStateUpdate::Duplicate(duplicate_state) => duplicate_state.bank_status.bank_hash(),
            SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state) => {
                epoch_slots_frozen_state.bank_status.bank_hash()
            }
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum ResultingStateChange {
    // Bank was frozen
    BankFrozen(Hash),
    // Hash of our current frozen version of the slot
    MarkSlotDuplicate(Hash),
    // Hash of the either:
    // 1) Cluster duplicate confirmed slot
    // 2) Epoch Slots frozen sampled slot
    // that is not equivalent to our frozen version of the slot
    RepairDuplicateConfirmedVersion(Hash),
    // Hash of our current frozen version of the slot
    DuplicateConfirmedSlotMatchesCluster(Hash),
    SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate),
}

impl SlotStateUpdate {
    // pub for tests
    pub fn into_state_changes(self, slot: Slot) -> Vec<ResultingStateChange> {
        let bank_frozen_hash = self.bank_hash();
        if bank_frozen_hash.is_none() {
            // If the bank hasn't been frozen yet, then there's nothing to do
            // since replay of the slot hasn't finished yet.
            return vec![];
        }

        match self {
            SlotStateUpdate::Dead(dead_state) => on_dead_slot(slot, dead_state),
            SlotStateUpdate::BankFrozen(bank_frozen_state) => {
                on_frozen_slot(slot, bank_frozen_state)
            }
            SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state) => {
                on_duplicate_confirmed(slot, duplicate_confirmed_state)
            }
            SlotStateUpdate::Duplicate(duplicate_state) => on_duplicate(duplicate_state),
            SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state) => {
                on_epoch_slots_frozen(slot, epoch_slots_frozen_state)
            }
        }
    }
}

pub fn check_duplicate_confirmed_hash_against_frozen_hash(
    state_changes: &mut Vec<ResultingStateChange>,
    slot: Slot,
    duplicate_confirmed_hash: Hash,
    bank_frozen_hash: Hash,
    is_dead: bool,
) {
    if duplicate_confirmed_hash != bank_frozen_hash {
        if is_dead {
            // If the cluster duplicate confirmed some version of this slot, then
            // there's another version of our dead slot
            warn!(
                "Cluster duplicate confirmed slot {} with hash {}, but we marked slot dead",
                slot, duplicate_confirmed_hash
            );
        } else {
            // The duplicate confirmed slot hash does not match our frozen hash.
            // Modify fork choice rule to exclude our version from being voted
            // on and also repair the correct version
            warn!(
                "Cluster duplicate confirmed slot {} with hash {}, but our version has hash {}",
                slot, duplicate_confirmed_hash, bank_frozen_hash
            );
        }
        state_changes.push(ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash));
        state_changes.push(ResultingStateChange::RepairDuplicateConfirmedVersion(
            duplicate_confirmed_hash,
        ));
    } else {
        // If the versions match, then add the slot to the candidate
        // set to account for the case where it was removed earlier
        // by the `on_duplicate_slot()` handler
        state_changes.push(ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(
            bank_frozen_hash,
        ));
    }
}

fn check_epoch_slots_hash_against_frozen_hash(
    state_changes: &mut Vec<ResultingStateChange>,
    slot: Slot,
    epoch_slots_frozen_hash: Hash,
    bank_frozen_hash: Hash,
    is_dead: bool,
) {
    if epoch_slots_frozen_hash != bank_frozen_hash {
        if is_dead {
            // If the cluster duplicate confirmed some version of this slot, then
            // there's another version of our dead slot
            warn!(
                "EpochSlots sample returned slot {} with hash {}, but we marked slot dead",
                slot, epoch_slots_frozen_hash
            );
        } else {
            // The duplicate confirmed slot hash does not match our frozen hash.
            // Modify fork choice rule to exclude our version from being voted
            // on and also repair the correct version
            warn!(
                "EpochSlots sample returned slot {} with hash {}, but our version
                has hash {}",
                slot, epoch_slots_frozen_hash, bank_frozen_hash
            );
        }
        state_changes.push(ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash));
        state_changes.push(ResultingStateChange::RepairDuplicateConfirmedVersion(
            epoch_slots_frozen_hash,
        ));
    }
}

fn on_dead_slot(slot: Slot, dead_state: DeadState) -> Vec<ResultingStateChange> {
    let DeadState {
        cluster_confirmed_hash,
        is_slot_duplicate,
    } = dead_state;

    let mut state_changes = vec![];
    if let Some(cluster_confirmed_hash) = cluster_confirmed_hash {
        match cluster_confirmed_hash {
            ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash) => {
                // If the cluster duplicate_confirmed some version of this slot, then
                // check if our version agrees with the cluster,
                let bank_hash = Hash::default();
                let is_dead = true;
                state_changes.push(ResultingStateChange::SendAncestorHashesReplayUpdate(
                    AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot),
                ));
                check_duplicate_confirmed_hash_against_frozen_hash(
                    &mut state_changes,
                    slot,
                    duplicate_confirmed_hash,
                    bank_hash,
                    is_dead,
                );
            }
            ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash) => {
                // Lower priority than having seen an actual duplicate confirmed hash in the
                // match arm above.
                let bank_hash = Hash::default();
                let is_dead = true;
                check_epoch_slots_hash_against_frozen_hash(
                    &mut state_changes,
                    slot,
                    epoch_slots_frozen_hash,
                    bank_hash,
                    is_dead,
                );
            }
        }
    } else {
        state_changes.push(ResultingStateChange::SendAncestorHashesReplayUpdate(
            AncestorHashesReplayUpdate::Dead(slot),
        ));
        if is_slot_duplicate {
            state_changes.push(ResultingStateChange::MarkSlotDuplicate(Hash::default()));
        }
    }

    state_changes
}

fn on_frozen_slot(slot: Slot, bank_frozen_state: BankFrozenState) -> Vec<ResultingStateChange> {
    let BankFrozenState {
        frozen_hash,
        cluster_confirmed_hash,
        is_slot_duplicate,
    } = bank_frozen_state;
    let mut state_changes = vec![ResultingStateChange::BankFrozen(frozen_hash)];
    if let Some(cluster_confirmed_hash) = cluster_confirmed_hash {
        match cluster_confirmed_hash {
            ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash) => {
                // If the cluster duplicate_confirmed some version of this slot, then
                // check if our version agrees with the cluster,
                let is_dead = false;
                check_duplicate_confirmed_hash_against_frozen_hash(
                    &mut state_changes,
                    slot,
                    duplicate_confirmed_hash,
                    frozen_hash,
                    is_dead,
                );
            }
            ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash) => {
                // Lower priority than having seen an actual duplicate confirmed hash in the
                // match arm above.
                let is_dead = false;
                check_epoch_slots_hash_against_frozen_hash(
                    &mut state_changes,
                    slot,
                    epoch_slots_frozen_hash,
                    frozen_hash,
                    is_dead,
                );
            }
        }
    }
    // If `cluster_confirmed_hash` is Some above we should have already pushed a
    // `MarkSlotDuplicate` state change
    else if is_slot_duplicate {
        state_changes.push(ResultingStateChange::MarkSlotDuplicate(frozen_hash));
    }

    state_changes
}

fn on_duplicate_confirmed(
    slot: Slot,
    duplicate_confirmed_state: DuplicateConfirmedState,
) -> Vec<ResultingStateChange> {
    let DuplicateConfirmedState {
        bank_status,
        duplicate_confirmed_hash,
    } = duplicate_confirmed_state;

    match bank_status {
        BankStatus::Dead | BankStatus::Frozen(_) => (),
        // No action to be taken yet
        BankStatus::Unprocessed => {
            return vec![];
        }
    }

    let bank_hash = bank_status.bank_hash().expect("bank hash must exist");

    let mut state_changes = vec![];
    let is_dead = bank_status.is_dead();
    if is_dead {
        state_changes.push(ResultingStateChange::SendAncestorHashesReplayUpdate(
            AncestorHashesReplayUpdate::DeadDuplicateConfirmed(slot),
        ));
    }
    check_duplicate_confirmed_hash_against_frozen_hash(
        &mut state_changes,
        slot,
        duplicate_confirmed_hash,
        bank_hash,
        is_dead,
    );

    state_changes
}

fn on_duplicate(duplicate_state: DuplicateState) -> Vec<ResultingStateChange> {
    let DuplicateState {
        bank_status,
        duplicate_confirmed_hash,
    } = duplicate_state;

    match bank_status {
        BankStatus::Dead | BankStatus::Frozen(_) => (),
        // No action to be taken yet
        BankStatus::Unprocessed => {
            return vec![];
        }
    }

    let bank_hash = bank_status.bank_hash().expect("bank hash must exist");

    // If the cluster duplicate_confirmed some version of this slot
    // then either the `SlotStateUpdate::DuplicateConfirmed`, `SlotStateUpdate::BankFrozen`,
    // or `SlotStateUpdate::Dead` state transitions will take care of marking the fork as
    // duplicate if there's a mismatch with our local version.
    if duplicate_confirmed_hash.is_none() {
        // If we have not yet seen any version of the slot duplicate confirmed, then mark
        // the slot as duplicate
        return vec![ResultingStateChange::MarkSlotDuplicate(bank_hash)];
    }

    vec![]
}

fn on_epoch_slots_frozen(
    slot: Slot,
    epoch_slots_frozen_state: EpochSlotsFrozenState,
) -> Vec<ResultingStateChange> {
    let EpochSlotsFrozenState {
        bank_status,
        epoch_slots_frozen_hash,
        duplicate_confirmed_hash,
    } = epoch_slots_frozen_state;

    if let Some(duplicate_confirmed_hash) = duplicate_confirmed_hash {
        if epoch_slots_frozen_hash != duplicate_confirmed_hash {
            warn!(
                "EpochSlots sample returned slot {} with hash {}, but we already saw
                duplicate confirmation on hash: {:?}",
                slot, epoch_slots_frozen_hash, duplicate_confirmed_hash
            );
        }
        return vec![];
    }

    match bank_status {
        BankStatus::Dead | BankStatus::Frozen(_) => (),
        // No action to be taken yet
        BankStatus::Unprocessed => {
            return vec![];
        }
    }

    let frozen_hash = bank_status.bank_hash().expect("bank hash must exist");
    let is_dead = bank_status.is_dead();
    let mut state_changes = vec![];
    check_epoch_slots_hash_against_frozen_hash(
        &mut state_changes,
        slot,
        epoch_slots_frozen_hash,
        frozen_hash,
        is_dead,
    );

    state_changes
}

fn get_cluster_confirmed_hash_from_state(
    slot: Slot,
    gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
    epoch_slots_frozen_slots: &EpochSlotsFrozenSlots,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    bank_frozen_hash: Option<Hash>,
) -> Option<ClusterConfirmedHash> {
    let gossip_duplicate_confirmed_hash = gossip_duplicate_confirmed_slots.get(&slot).cloned();
    // If the bank hasn't been frozen yet, then we haven't duplicate confirmed a local version
    // this slot through replay yet.
    let is_local_replay_duplicate_confirmed = if let Some(bank_frozen_hash) = bank_frozen_hash {
        fork_choice
            .is_duplicate_confirmed(&(slot, bank_frozen_hash))
            .unwrap_or(false)
    } else {
        false
    };

    get_duplicate_confirmed_hash(
        slot,
        gossip_duplicate_confirmed_hash,
        bank_frozen_hash,
        is_local_replay_duplicate_confirmed,
    )
    .map(ClusterConfirmedHash::DuplicateConfirmed)
    .or_else(|| {
        epoch_slots_frozen_slots
            .get(&slot)
            .map(|hash| ClusterConfirmedHash::EpochSlotsFrozen(*hash))
    })
}

fn get_duplicate_confirmed_hash_from_state(
    slot: Slot,
    gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    bank_frozen_hash: Option<Hash>,
) -> Option<Hash> {
    let gossip_duplicate_confirmed_hash = gossip_duplicate_confirmed_slots.get(&slot).cloned();
    // If the bank hasn't been frozen yet, then we haven't duplicate confirmed a local version
    // this slot through replay yet.
    let is_local_replay_duplicate_confirmed = if let Some(bank_frozen_hash) = bank_frozen_hash {
        fork_choice
            .is_duplicate_confirmed(&(slot, bank_frozen_hash))
            .unwrap_or(false)
    } else {
        false
    };

    get_duplicate_confirmed_hash(
        slot,
        gossip_duplicate_confirmed_hash,
        bank_frozen_hash,
        is_local_replay_duplicate_confirmed,
    )
}

fn get_duplicate_confirmed_hash(
    slot: Slot,
    gossip_duplicate_confirmed_hash: Option<Hash>,
    bank_frozen_hash: Option<Hash>,
    is_local_replay_duplicate_confirmed: bool,
) -> Option<Hash> {
    let local_duplicate_confirmed_hash = if is_local_replay_duplicate_confirmed {
        // If local replay has duplicate_confirmed this slot, this slot must have
        // descendants with votes for this slot, hence this slot must be
        // frozen.
        let bank_frozen_hash = bank_frozen_hash.unwrap();
        assert!(bank_frozen_hash != Hash::default());
        Some(bank_frozen_hash)
    } else {
        None
    };

    match (
        local_duplicate_confirmed_hash,
        gossip_duplicate_confirmed_hash,
    ) {
        (Some(local_duplicate_confirmed_hash), Some(gossip_duplicate_confirmed_hash)) => {
            if local_duplicate_confirmed_hash != gossip_duplicate_confirmed_hash {
                error!(
                    "For slot {}, the gossip duplicate confirmed hash {}, is not equal
                to the confirmed hash we replayed: {}",
                    slot, gossip_duplicate_confirmed_hash, local_duplicate_confirmed_hash
                );
            }
            Some(local_duplicate_confirmed_hash)
        }
        (Some(bank_frozen_hash), None) => Some(bank_frozen_hash),
        _ => gossip_duplicate_confirmed_hash,
    }
}

// pub for tests
pub fn apply_state_changes(
    slot: Slot,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
    blockstore: &Blockstore,
    ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
    purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    state_changes: Vec<ResultingStateChange>,
) {
    // Handle cases where the bank is frozen, but not duplicate confirmed
    // yet.
    let mut not_duplicate_confirmed_frozen_hash = None;
    for state_change in state_changes {
        match state_change {
            ResultingStateChange::BankFrozen(bank_frozen_hash) => {
                if !fork_choice
                    .is_duplicate_confirmed(&(slot, bank_frozen_hash))
                    .expect("frozen bank must exist in fork choice")
                {
                    not_duplicate_confirmed_frozen_hash = Some(bank_frozen_hash);
                }
            }
            ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash) => {
                fork_choice.mark_fork_invalid_candidate(&(slot, bank_frozen_hash));
            }
            ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash) => {
                duplicate_slots_to_repair.insert(slot, duplicate_confirmed_hash);
            }
            ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(bank_frozen_hash) => {
                not_duplicate_confirmed_frozen_hash = None;
                // When we detect that our frozen slot matches the cluster version (note this
                // will catch both bank frozen first -> confirmation, or confirmation first ->
                // bank frozen), mark all the newly duplicate confirmed slots in blockstore
                let new_duplicate_confirmed_slot_hashes =
                    fork_choice.mark_fork_valid_candidate(&(slot, bank_frozen_hash));
                blockstore
                    .set_duplicate_confirmed_slots_and_hashes(
                        new_duplicate_confirmed_slot_hashes.into_iter(),
                    )
                    .unwrap();
                duplicate_slots_to_repair.remove(&slot);
                purge_repair_slot_counter.remove(&slot);
            }
            ResultingStateChange::SendAncestorHashesReplayUpdate(ancestor_hashes_replay_update) => {
                let _ = ancestor_hashes_replay_update_sender.send(ancestor_hashes_replay_update);
            }
        }
    }

    if let Some(frozen_hash) = not_duplicate_confirmed_frozen_hash {
        blockstore.insert_bank_hash(slot, frozen_hash, false);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn check_slot_agrees_with_cluster(
    slot: Slot,
    root: Slot,
    blockstore: &Blockstore,
    duplicate_slots_tracker: &mut DuplicateSlotsTracker,
    epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
    ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
    purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    slot_state_update: SlotStateUpdate,
) {
    info!(
        "check_slot_agrees_with_cluster() slot: {}, root: {}, slot_state_update: {:?}",
        slot, root, slot_state_update
    );

    if slot <= root {
        return;
    }

    // Needs to happen before the bank_frozen_hash.is_none() check below to account for duplicate
    // signals arriving before the bank is constructed in replay.
    if let SlotStateUpdate::Duplicate(ref state) = slot_state_update {
        // If this slot has already been processed before, return
        if !duplicate_slots_tracker.insert(slot) {
            return;
        }

        datapoint_info!(
            "duplicate_slot",
            ("slot", slot, i64),
            (
                "duplicate_confirmed_hash",
                state
                    .duplicate_confirmed_hash
                    .unwrap_or_default()
                    .to_string(),
                String
            ),
            (
                "my_hash",
                state
                    .bank_status
                    .bank_hash()
                    .unwrap_or_default()
                    .to_string(),
                String
            ),
        );
    }

    // Avoid duplicate work from multiple of the same DuplicateConfirmed signal. This can
    // happen if we get duplicate confirmed from gossip and from local replay.
    if let SlotStateUpdate::DuplicateConfirmed(state) = &slot_state_update {
        if let Some(bank_hash) = state.bank_status.bank_hash() {
            if let Some(true) = fork_choice.is_duplicate_confirmed(&(slot, bank_hash)) {
                return;
            }
        }

        datapoint_info!(
            "duplicate_confirmed_slot",
            ("slot", slot, i64),
            (
                "duplicate_confirmed_hash",
                state.duplicate_confirmed_hash.to_string(),
                String
            ),
            (
                "my_hash",
                state
                    .bank_status
                    .bank_hash()
                    .unwrap_or_default()
                    .to_string(),
                String
            ),
        );
    }

    if let SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state) = &slot_state_update {
        if let Some(old_epoch_slots_frozen_hash) =
            epoch_slots_frozen_slots.insert(slot, epoch_slots_frozen_state.epoch_slots_frozen_hash)
        {
            if old_epoch_slots_frozen_hash == epoch_slots_frozen_state.epoch_slots_frozen_hash {
                // If EpochSlots has already told us this same hash was frozen, return
                return;
            }
        }
    }

    let state_changes = slot_state_update.into_state_changes(slot);
    apply_state_changes(
        slot,
        fork_choice,
        duplicate_slots_to_repair,
        blockstore,
        ancestor_hashes_replay_update_sender,
        purge_repair_slot_counter,
        state_changes,
    );
}

#[cfg(test)]
mod test {
    use super::*;

    macro_rules! state_update_tests {
        ($($name:ident: $value:expr,)*) => {
        $(
            #[test]
            fn $name() {
                let slot = 10;
                let (state_update, expected) = $value;
                assert_eq!(expected, state_update.into_state_changes(slot));
            }
        )*
        }
    }

    state_update_tests! {
        bank_frozen_state_update_0: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = None;
            let is_slot_duplicate = false;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash)]
            )
        },
        bank_frozen_state_update_1: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = None;
            let is_slot_duplicate = true;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash), ResultingStateChange::MarkSlotDuplicate(frozen_hash)]
            )
        },
        bank_frozen_state_update_2: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(frozen_hash));
            let is_slot_duplicate = false;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(frozen_hash)]
            )
        },
        bank_frozen_state_update_3: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(frozen_hash));
            let is_slot_duplicate = false;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash)]
            )
        },
        bank_frozen_state_update_4: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(frozen_hash));
            let is_slot_duplicate = true;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(frozen_hash)]
            )
        },
        bank_frozen_state_update_5: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(frozen_hash));
            let is_slot_duplicate = true;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash)],
            )
        },
        bank_frozen_state_update_6: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash));
            let is_slot_duplicate = false;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        bank_frozen_state_update_7: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let epoch_slots_frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash));
            let is_slot_duplicate = false;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        bank_frozen_state_update_8: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash));
            let is_slot_duplicate = true;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        bank_frozen_state_update_9: {
            // frozen hash has to be non-default for frozen state transition
            let frozen_hash = Hash::new_unique();
            let epoch_slots_frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash));
            let is_slot_duplicate = true;
            let bank_frozen_state = BankFrozenState::new(
                frozen_hash,
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::BankFrozen(bank_frozen_state),
                vec![ResultingStateChange::BankFrozen(frozen_hash),
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        duplicate_confirmed_state_update_0: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let bank_status = BankStatus::Unprocessed;
            let duplicate_confirmed_state = DuplicateConfirmedState::new(
                duplicate_confirmed_hash,
                bank_status,
            );
            (
                SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        duplicate_confirmed_state_update_1: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let bank_status = BankStatus::Dead;
            let duplicate_confirmed_state = DuplicateConfirmedState::new(
                duplicate_confirmed_hash,
                bank_status,
            );
            (
                SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                vec![
                ResultingStateChange::SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(10)),
                ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        duplicate_confirmed_state_update_2: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let bank_status = BankStatus::Frozen(duplicate_confirmed_hash);
            let duplicate_confirmed_state = DuplicateConfirmedState::new(
                duplicate_confirmed_hash,
                bank_status,
            );
            (
                SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                vec![
                ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(duplicate_confirmed_hash)]
            )
        },
        duplicate_confirmed_state_update_3: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let frozen_hash = Hash::new_unique();
            let bank_status = BankStatus::Frozen(frozen_hash);
            let duplicate_confirmed_state = DuplicateConfirmedState::new(
                duplicate_confirmed_hash,
                bank_status,
            );
            (
                SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                vec![
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        dead_state_update_0: {
            let cluster_confirmed_hash = None;
            let is_slot_duplicate = false;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![
                    ResultingStateChange::SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate::Dead(10))
                ],
            )
        },
        dead_state_update_1: {
            let cluster_confirmed_hash = None;
            let is_slot_duplicate = true;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![
                    ResultingStateChange::SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate::Dead(10)), ResultingStateChange::MarkSlotDuplicate(Hash::default())],
            )
        },
        dead_state_update_2: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash));
            let is_slot_duplicate = false;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![
                ResultingStateChange::SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(10)),
                ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        dead_state_update_3: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash));
            let is_slot_duplicate = false;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![
                ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        dead_state_update_4: {
            let duplicate_confirmed_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::DuplicateConfirmed(duplicate_confirmed_hash));
            let is_slot_duplicate = true;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![

                    ResultingStateChange::SendAncestorHashesReplayUpdate(AncestorHashesReplayUpdate::DeadDuplicateConfirmed(10)),
                ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                ResultingStateChange::RepairDuplicateConfirmedVersion(duplicate_confirmed_hash)],
            )
        },
        dead_state_update_5: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let cluster_confirmed_hash = Some(ClusterConfirmedHash::EpochSlotsFrozen(epoch_slots_frozen_hash));
            let is_slot_duplicate = true;
            let dead_state = DeadState::new(
                cluster_confirmed_hash,
                is_slot_duplicate,
            );
            (
                SlotStateUpdate::Dead(dead_state),
                vec![
                    ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                    ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        duplicate_state_update_0: {
            let duplicate_confirmed_hash = None;
            let bank_status = BankStatus::Unprocessed;
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        duplicate_state_update_1: {
            let duplicate_confirmed_hash = None;
            let bank_status = BankStatus::Dead;
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                vec![ResultingStateChange::MarkSlotDuplicate(Hash::default())],
            )
        },
        duplicate_state_update_2: {
            let duplicate_confirmed_hash = None;
            let bank_hash = Hash::new_unique();
            let bank_status = BankStatus::Frozen(bank_hash);
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                vec![ResultingStateChange::MarkSlotDuplicate(bank_hash)],
            )
        },
        duplicate_state_update_3: {
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Unprocessed;
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                Vec::<ResultingStateChange>::new(),
            )
        },
        duplicate_state_update_4: {
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Dead;
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        duplicate_state_update_5: {
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Frozen(duplicate_confirmed_hash.unwrap());
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        duplicate_state_update_6: {
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let frozen_hash = Hash::new_unique();
            let bank_status = BankStatus::Frozen(frozen_hash);
            let duplicate_state = DuplicateState::new(duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::Duplicate(duplicate_state),
                Vec::<ResultingStateChange>::new(),
            )
        },
        epoch_slots_frozen_state_update_0: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = None;
            let bank_status = BankStatus::Unprocessed;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_1: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Unprocessed;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_2: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(epoch_slots_frozen_hash);
            let bank_status = BankStatus::Unprocessed;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_3: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = None;
            let bank_status = BankStatus::Dead;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                vec![
                ResultingStateChange::MarkSlotDuplicate(Hash::default()),
                ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        epoch_slots_frozen_state_update_4: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Dead;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_5: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(epoch_slots_frozen_hash);
            let bank_status = BankStatus::Dead;
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_6: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = None;
            let frozen_hash = Hash::new_unique();
            let bank_status = BankStatus::Frozen(frozen_hash);
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                vec![
                ResultingStateChange::MarkSlotDuplicate(frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(epoch_slots_frozen_hash)],
            )
        },
        epoch_slots_frozen_state_update_7: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = None;
            let bank_status = BankStatus::Frozen(epoch_slots_frozen_hash);
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_8: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Frozen(Hash::new_unique());
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_9: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Frozen(epoch_slots_frozen_hash);
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
        epoch_slots_frozen_state_update_10: {
            let epoch_slots_frozen_hash = Hash::new_unique();
            let duplicate_confirmed_hash = Some(Hash::new_unique());
            let bank_status = BankStatus::Frozen(duplicate_confirmed_hash.unwrap());
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new(epoch_slots_frozen_hash, duplicate_confirmed_hash, bank_status);
            (
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
                Vec::<ResultingStateChange>::new()
            )
        },
    }
}
