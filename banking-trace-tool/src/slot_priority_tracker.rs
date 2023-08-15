use {
    crate::process::process_event_files,
    chrono::{DateTime, Utc},
    clap::ValueEnum,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_sdk::{pubkey::Pubkey, signature::Signature},
    std::{
        cmp::Ordering,
        collections::{BTreeSet, HashMap, HashSet},
        path::PathBuf,
    },
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum TrackingKind {
    /// Determine the highest-priority write account for each slot.
    HighestPriorityAccount,
    /// Determine the highest-conflict write account for each slot.
    HighestConflictAccount,
    /// Log unique lookup-tables accessed by each slot.
    LookupTables,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum TrackingVerbosity {
    /// Simple logging without significant detail.
    #[default]
    Simple,
    /// Show top 10 transactions in the appropriate tree.
    TreeTop10,
    /// Show top 10 accounts for appropriate tracking kind.
    AccountTop10,
}

impl std::fmt::Display for TrackingVerbosity {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TrackingVerbosity::Simple => write!(f, "simple"),
            TrackingVerbosity::TreeTop10 => write!(f, "tree-top10"),
            TrackingVerbosity::AccountTop10 => write!(f, "account-top10"),
        }
    }
}

pub fn do_slot_priority_tracking(
    event_file_paths: &[PathBuf],
    kind: TrackingKind,
    verbosity: TrackingVerbosity,
) -> std::io::Result<()> {
    let mut slot_priority_tracker = SlotPriorityTracker::default();
    let mut handler = |event| slot_priority_tracker.process_event(event);
    process_event_files(event_file_paths, &mut handler)?;

    slot_priority_tracker.report(kind, verbosity);

    Ok(())
}

#[derive(Default)]
pub struct SlotPriorityTracker {
    /// Data for each slot
    data: Vec<SlotPriorityData>,
    /// The current slot data
    current_slot_data: SlotPriorityData,
}

impl SlotPriorityTracker {
    pub fn report(&self, kind: TrackingKind, verbosity: TrackingVerbosity) {
        for slot_data in &self.data {
            slot_data.report(kind, verbosity);
            break;
        }
    }

    pub fn process_event(&mut self, TimedTracedEvent(timestamp, event): TimedTracedEvent) {
        match event {
            TracedEvent::PacketBatch(label, banking_packet_batch) => {
                // TODO: Pass filter as param.
                if !matches!(label, ChannelLabel::NonVote) {
                    return;
                }
                if banking_packet_batch.0.is_empty() {
                    return;
                }

                let utc = DateTime::<Utc>::from(timestamp);
                let packets = banking_packet_batch
                    .0
                    .iter()
                    .flatten()
                    .cloned()
                    .filter_map(|p| ImmutableDeserializedPacket::new(p).ok())
                    .collect();
                self.process_packets(utc, packets);
            }
            TracedEvent::BlockAndBankHash(slot, _, _) => {
                self.current_slot_data.slot = slot;
                self.data.push(core::mem::take(&mut self.current_slot_data));
            }
        }
    }

    fn process_packets(
        &mut self,
        timestamp: DateTime<Utc>,
        packets: Vec<ImmutableDeserializedPacket>,
    ) {
        for packet in packets {
            self.current_slot_data.process_message(timestamp, packet);
        }
    }
}

#[derive(Default)]
pub struct SlotPriorityData {
    /// The slot number
    slot: u64, // is not filled in until end of slot
    /// Account look-up tables accessed by the slot
    account_lookups: HashSet<Pubkey>,
    /// Time-ordered list of transaction signatures for each account.
    time_ordered_transactions_by_account:
        HashMap<Pubkey, BTreeSet<TimeOrderedTransactionSignature>>,
    /// Priority-ordered list of transaction signatures for each account.
    priority_ordered_transactions_by_account:
        HashMap<Pubkey, BTreeSet<PriorityOrderedTransactionSignature>>,

    sigmap: HashMap<Signature, ImmutableDeserializedPacket>,
}

impl SlotPriorityData {
    fn report(&self, kind: TrackingKind, verbosity: TrackingVerbosity) {
        match kind {
            TrackingKind::HighestPriorityAccount => {
                self.report_highest_priority_account(verbosity);
            }
            TrackingKind::HighestConflictAccount => {
                self.report_highest_conflict_account(verbosity);
            }
            TrackingKind::LookupTables => {
                self.report_lookup_tables(verbosity);
            }
        }
    }

    fn report_highest_priority_account(&self, verbosity: TrackingVerbosity) {
        let mut priority_ordered_trees = self
            .priority_ordered_transactions_by_account
            .iter()
            .clone()
            .collect::<Vec<_>>();
        priority_ordered_trees.sort_by(|a, b| {
            b.1.first()
                .unwrap()
                .priority
                .cmp(&a.1.first().unwrap().priority)
        });

        let slot = self.slot;
        match verbosity {
            TrackingVerbosity::Simple => {
                let (highest_priority_account, tree) = priority_ordered_trees.first().unwrap();
                let highest_priority = tree.first().unwrap().priority;
                let num_writes = tree.len();
                println!("{slot}: {highest_priority_account} highest_priority={highest_priority} num_writes={num_writes}")
            }
            TrackingVerbosity::TreeTop10 => {
                let (highest_priority_account, tree) = priority_ordered_trees.first().unwrap();
                let num_writes = tree.len();
                let top_txs: Vec<_> = tree.iter().take(10).map(|tx| tx.priority).collect();
                let pretty_top_txs = top_txs.pretty();
                println!("{slot}: {highest_priority_account} num_writes={num_writes} top_txs={pretty_top_txs}")
            }
            TrackingVerbosity::AccountTop10 => {
                let top_accounts = priority_ordered_trees
                    .into_iter()
                    .take(10)
                    .map(TreeTopSummary::from)
                    .collect::<Vec<_>>();
                println!("{slot}: top_accounts={}", top_accounts.pretty());
            }
        }
    }

    fn report_highest_conflict_account(&self, verbosity: TrackingVerbosity) {
        let mut conflict_ordered_trees = self
            .priority_ordered_transactions_by_account
            .iter()
            .clone()
            .collect::<Vec<_>>();
        conflict_ordered_trees.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let slot = self.slot;
        match verbosity {
            TrackingVerbosity::Simple => {
                let (highest_conflict_account, tree) = conflict_ordered_trees.first().unwrap();
                let highest_priority = tree.first().unwrap().priority;
                let num_writes = tree.len();
                println!("{slot}: {highest_conflict_account} highest_priority={highest_priority} num_writes={num_writes}")
            }
            TrackingVerbosity::TreeTop10 => {
                let (highest_conflict_account, tree) = conflict_ordered_trees.first().unwrap();
                let num_writes = tree.len();
                let top_txs: Vec<_> = tree.iter().take(10).map(|tx| tx.priority).collect();
                let pretty_top_txs = top_txs.pretty();
                println!("{slot}: {highest_conflict_account} num_writes={num_writes} top_txs={pretty_top_txs}")
            }
            TrackingVerbosity::AccountTop10 => {
                let top_accounts = conflict_ordered_trees
                    .into_iter()
                    .take(10)
                    .map(TreeTopSummary::from)
                    .collect::<Vec<_>>();
                println!("{slot}: top_accounts={}", top_accounts.pretty());
            }
        }
    }

    fn report_lookup_tables(&self, verbosity: TrackingVerbosity) {
        let slot = self.slot;
        let mut lookup_tables = self.account_lookups.iter().cloned().collect::<Vec<_>>();
        lookup_tables.sort_unstable();
        match verbosity {
            TrackingVerbosity::Simple
            | TrackingVerbosity::TreeTop10
            | TrackingVerbosity::AccountTop10 => {
                println!("{slot}: {lookup_tables:?}")
            }
        }
    }

    fn process_message(&mut self, timestamp: DateTime<Utc>, packet: ImmutableDeserializedPacket) {
        let Some(signature) = packet.transaction().get_signatures().first().cloned() else {
            return;
        };

        // Collect all accessed lookup-tables
        let priority = packet.priority();
        let message = &packet.transaction().get_message().message;
        if let Some(address_lookup_tables) = message.address_table_lookups() {
            self.account_lookups
                .extend(address_lookup_tables.iter().map(|x| x.account_key));
        }

        // Insert for each static account map
        let fee_payer = *message.static_account_keys().first().unwrap();
        for (index, account) in message.static_account_keys().iter().enumerate() {
            if !message.is_maybe_writable(index) {
                continue;
            }
            self.time_ordered_transactions_by_account
                .entry(*account)
                .or_default()
                .insert(TimeOrderedTransactionSignature {
                    timestamp,
                    priority,
                    signature,
                    fee_payer,
                });
            self.priority_ordered_transactions_by_account
                .entry(*account)
                .or_default()
                .insert(PriorityOrderedTransactionSignature {
                    timestamp,
                    priority,
                    signature,
                    fee_payer,
                });
        }
        self.sigmap.insert(signature, packet);
    }
}

#[derive(PartialEq, Eq)]
pub struct TimeOrderedTransactionSignature {
    timestamp: DateTime<Utc>,
    priority: u64,
    signature: Signature,
    fee_payer: Pubkey,
}

impl PartialOrd for TimeOrderedTransactionSignature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.timestamp.partial_cmp(&other.timestamp)
    }
}

impl Ord for TimeOrderedTransactionSignature {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

#[derive(PartialEq, Eq)]
pub struct PriorityOrderedTransactionSignature {
    timestamp: DateTime<Utc>,
    priority: u64,
    signature: Signature,
    fee_payer: Pubkey,
}

impl std::fmt::Display for PriorityOrderedTransactionSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:} {:} ({:} - {:})",
            self.signature, self.fee_payer, self.timestamp, self.priority
        )
    }
}

impl PartialOrd for PriorityOrderedTransactionSignature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.priority.partial_cmp(&other.priority)
    }
}

impl Ord for PriorityOrderedTransactionSignature {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

impl std::fmt::Display for TimeOrderedTransactionSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:} {:} ({:} - {:})",
            self.signature, self.fee_payer, self.timestamp, self.priority
        )
    }
}

/// Wrapper type for summarizing the top tx in a tree
struct TreeTopSummary {
    account: Pubkey,
    num_writes: usize,
    highest_priority: u64,
    top_fee_payer: Pubkey,
}

impl From<(&Pubkey, &BTreeSet<PriorityOrderedTransactionSignature>)> for TreeTopSummary {
    fn from((account, tree): (&Pubkey, &BTreeSet<PriorityOrderedTransactionSignature>)) -> Self {
        let first = tree.first().unwrap();
        let highest_priority = first.priority;
        let top_fee_payer = first.fee_payer;
        let num_writes = tree.len();
        Self {
            account: *account,
            num_writes,
            highest_priority,
            top_fee_payer,
        }
    }
}

impl std::fmt::Display for TreeTopSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{{account:} num_writes={num_writes} highest_priority={highest_priority} top_fee_payer={top_fee_payer:}}}",
            account = self.account,
            num_writes = self.num_writes,
            highest_priority = self.highest_priority,
            top_fee_payer = self.top_fee_payer,
        )
    }
}

pub struct PrettySlice<'a, T: std::fmt::Display>(&'a [T]);

impl<'a, T: std::fmt::Display> std::fmt::Display for PrettySlice<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for item in self.0 {
            write!(f, "{item:}, ")?;
        }
        write!(f, "]")?;
        Ok(())
    }
}

pub trait PrettySliceExt<'a, T: std::fmt::Display> {
    fn pretty(&'a self) -> PrettySlice<'a, T>;
}

impl<T: std::fmt::Display> PrettySliceExt<'_, T> for Vec<T> {
    fn pretty(&self) -> PrettySlice<T> {
        PrettySlice(self)
    }
}
