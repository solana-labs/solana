use {
    crate::process::process_event_files,
    chrono::{DateTime, Utc},
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

pub fn run(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut slot_priority_tracker = SlotPriorityTracker::default();
    let mut handler = |event| slot_priority_tracker.process_event(event);
    process_event_files(event_file_paths, &mut handler)?;

    slot_priority_tracker.report();

    Ok(())
}

#[derive(Default)]
pub struct SlotPriorityTracker {
    /// Data for each slot
    data: Vec<SlotPriorityData>,
    /// The current slot
    current_slot: u64,
}

impl SlotPriorityTracker {
    pub fn report(&self) {
        for slot_data in &self.data {
            slot_data.report();
            // break; // debug
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
                if self.current_slot != slot {
                    self.current_slot = slot;
                    self.data.push(SlotPriorityData::new(slot));
                    println!("new slot: {slot}");
                }
            }
        }
    }

    fn process_packets(
        &mut self,
        timestamp: DateTime<Utc>,
        packets: Vec<ImmutableDeserializedPacket>,
    ) {
        let Some(slot_data) = self.data.last_mut() else {
            return;
        };
        for packet in packets {
            slot_data.process_message(timestamp, packet);
        }
    }
}

pub struct SlotPriorityData {
    /// The slot number
    slot: u64,
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
    fn new(slot: u64) -> Self {
        Self {
            slot,
            account_lookups: HashSet::new(),
            time_ordered_transactions_by_account: HashMap::new(),
            priority_ordered_transactions_by_account: HashMap::new(),
            sigmap: HashMap::new(),
        }
    }

    fn report(&self) {
        println!("Slot: {}", self.slot);
        let account_lookups = self.account_lookups.iter().cloned().collect::<Vec<_>>();
        // println!("\tAccount lookups: {account_lookups:#?}");
        let account = self
            .time_ordered_transactions_by_account
            .iter()
            .max_by(|a, b| a.1.len().cmp(&b.1.len()))
            .unwrap()
            .0;
        println!("\tMost conflicting account: {account}");

        // // println!("\tAccount lookups: {account_lookups:#?}");
        // // for account in self.time_ordered_transactions_by_account.keys() {
        // let time_ordered = self
        //     .time_ordered_transactions_by_account
        //     .get(account)
        //     .unwrap()
        //     .into_iter()
        //     .collect::<Vec<_>>();
        // let priority_ordered = self
        //     .priority_ordered_transactions_by_account
        //     .get(account)
        //     .unwrap()
        //     .into_iter()
        //     .collect::<Vec<_>>();

        // //     println!("\tAccount {account}");
        // pretty_print("Time-ordered: ", &time_ordered);
        // pretty_print("Priority-ordered: ", &priority_ordered);

        // }
        // println!();
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
                .extend(address_lookup_tables.into_iter().map(|x| x.account_key));
        }

        // Insert for each static account map
        let fee_payer = message.static_account_keys().first().unwrap().clone();
        for (index, account) in message.static_account_keys().into_iter().enumerate() {
            if !message.is_maybe_writable(index) {
                continue;
            }
            self.time_ordered_transactions_by_account
                .entry(*account)
                .or_default()
                .insert(TimeOrderedTransactionSignature {
                    timestamp,
                    priority,
                    signature: signature.clone(),
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

#[derive(PartialEq, Eq, Ord)]
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

#[derive(PartialEq, Eq, Ord)]
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

impl std::fmt::Display for TimeOrderedTransactionSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:} {:} ({:} - {:})",
            self.signature, self.fee_payer, self.timestamp, self.priority
        )
    }
}

fn pretty_print<T: std::fmt::Display>(name: &str, items: &[T]) {
    println!("\t\t{name}: [");
    for item in items.iter() {
        println!("\t\t\t{item}");
    }
    println!("\t\t]");
}
