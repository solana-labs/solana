use {
    chrono::{Local, TimeZone},
    serde::{Deserialize, Serialize},
    solana_cli_output::{display::writeln_transaction, QuietDisplay, VerboseDisplay},
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        native_token::lamports_to_sol,
    },
    solana_transaction_status::{
        EncodedConfirmedBlock, EncodedTransactionWithStatusMeta, EntrySummary, Rewards,
    },
    std::fmt::{self, Display, Formatter, Result},
};

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotInfo {
    pub total: usize,
    pub first: Option<u64>,
    pub last: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_after_last_root: Option<usize>,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotBounds<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_slots: Option<&'a Vec<u64>>,
    pub slots: SlotInfo,
    pub roots: SlotInfo,
}

impl VerboseDisplay for SlotBounds<'_> {}
impl QuietDisplay for SlotBounds<'_> {}

impl Display for SlotBounds<'_> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if self.slots.total > 0 {
            let first = self.slots.first.unwrap();
            let last = self.slots.last.unwrap();

            if first != last {
                writeln!(
                    f,
                    "Ledger has data for {:?} slots {:?} to {:?}",
                    self.slots.total, first, last
                )?;

                if let Some(all_slots) = self.all_slots {
                    writeln!(f, "Non-empty slots: {all_slots:?}")?;
                }
            } else {
                writeln!(f, "Ledger has data for slot {first:?}")?;
            }

            if self.roots.total > 0 {
                let first_rooted = self.roots.first.unwrap_or_default();
                let last_rooted = self.roots.last.unwrap_or_default();
                let num_after_last_root = self.roots.num_after_last_root.unwrap_or_default();
                writeln!(
                    f,
                    "  with {:?} rooted slots from {:?} to {:?}",
                    self.roots.total, first_rooted, last_rooted
                )?;

                writeln!(f, "  and {num_after_last_root:?} slots past the last root")?;
            } else {
                writeln!(f, "  with no rooted slots")?;
            }
        } else {
            writeln!(f, "Ledger is empty")?;
        }

        Ok(())
    }
}

fn writeln_entry(f: &mut dyn fmt::Write, i: usize, entry: &CliEntry, prefix: &str) -> fmt::Result {
    writeln!(
        f,
        "{prefix}Entry {} - num_hashes: {}, hash: {}, transactions: {}, starting_transaction_index: {}",
        i, entry.num_hashes, entry.hash, entry.num_transactions, entry.starting_transaction_index,
    )
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEntries {
    pub entries: Vec<CliEntry>,
    #[serde(skip_serializing)]
    pub slot: Slot,
}

impl QuietDisplay for CliEntries {}
impl VerboseDisplay for CliEntries {}

impl fmt::Display for CliEntries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Slot {}", self.slot)?;
        for (i, entry) in self.entries.iter().enumerate() {
            writeln_entry(f, i, entry, "  ")?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEntry {
    num_hashes: u64,
    hash: String,
    num_transactions: u64,
    starting_transaction_index: usize,
}

impl From<EntrySummary> for CliEntry {
    fn from(entry_summary: EntrySummary) -> Self {
        Self {
            num_hashes: entry_summary.num_hashes,
            hash: entry_summary.hash.to_string(),
            num_transactions: entry_summary.num_transactions,
            starting_transaction_index: entry_summary.starting_transaction_index,
        }
    }
}

impl From<&CliPopulatedEntry> for CliEntry {
    fn from(populated_entry: &CliPopulatedEntry) -> Self {
        Self {
            num_hashes: populated_entry.num_hashes,
            hash: populated_entry.hash.clone(),
            num_transactions: populated_entry.num_transactions,
            starting_transaction_index: populated_entry.starting_transaction_index,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliPopulatedEntry {
    num_hashes: u64,
    hash: String,
    num_transactions: u64,
    starting_transaction_index: usize,
    transactions: Vec<EncodedTransactionWithStatusMeta>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliBlockWithEntries {
    #[serde(flatten)]
    pub encoded_confirmed_block: EncodedConfirmedBlockWithEntries,
    #[serde(skip_serializing)]
    pub slot: Slot,
}

impl QuietDisplay for CliBlockWithEntries {}
impl VerboseDisplay for CliBlockWithEntries {}

impl fmt::Display for CliBlockWithEntries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Slot: {}", self.slot)?;
        writeln!(
            f,
            "Parent Slot: {}",
            self.encoded_confirmed_block.parent_slot
        )?;
        writeln!(f, "Blockhash: {}", self.encoded_confirmed_block.blockhash)?;
        writeln!(
            f,
            "Previous Blockhash: {}",
            self.encoded_confirmed_block.previous_blockhash
        )?;
        if let Some(block_time) = self.encoded_confirmed_block.block_time {
            writeln!(
                f,
                "Block Time: {:?}",
                Local.timestamp_opt(block_time, 0).unwrap()
            )?;
        }
        if let Some(block_height) = self.encoded_confirmed_block.block_height {
            writeln!(f, "Block Height: {block_height:?}")?;
        }
        if !self.encoded_confirmed_block.rewards.is_empty() {
            let mut rewards = self.encoded_confirmed_block.rewards.clone();
            rewards.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
            let mut total_rewards = 0;
            writeln!(f, "Rewards:")?;
            writeln!(
                f,
                "  {:<44}  {:^15}  {:<15}  {:<20}  {:>14}  {:>10}",
                "Address", "Type", "Amount", "New Balance", "Percent Change", "Commission"
            )?;
            for reward in rewards {
                let sign = if reward.lamports < 0 { "-" } else { "" };

                total_rewards += reward.lamports;
                #[allow(clippy::format_in_format_args)]
                writeln!(
                    f,
                    "  {:<44}  {:^15}  {:>15}  {}  {}",
                    reward.pubkey,
                    if let Some(reward_type) = reward.reward_type {
                        format!("{reward_type}")
                    } else {
                        "-".to_string()
                    },
                    format!(
                        "{}◎{:<14.9}",
                        sign,
                        lamports_to_sol(reward.lamports.unsigned_abs())
                    ),
                    if reward.post_balance == 0 {
                        "          -                 -".to_string()
                    } else {
                        format!(
                            "◎{:<19.9}  {:>13.9}%",
                            lamports_to_sol(reward.post_balance),
                            (reward.lamports.abs() as f64
                                / (reward.post_balance as f64 - reward.lamports as f64))
                                * 100.0
                        )
                    },
                    reward
                        .commission
                        .map(|commission| format!("{commission:>9}%"))
                        .unwrap_or_else(|| "    -".to_string())
                )?;
            }

            let sign = if total_rewards < 0 { "-" } else { "" };
            writeln!(
                f,
                "Total Rewards: {}◎{:<12.9}",
                sign,
                lamports_to_sol(total_rewards.unsigned_abs())
            )?;
        }
        for (index, entry) in self.encoded_confirmed_block.entries.iter().enumerate() {
            writeln_entry(f, index, &entry.into(), "")?;
            for (index, transaction_with_meta) in entry.transactions.iter().enumerate() {
                writeln!(f, "  Transaction {index}:")?;
                writeln_transaction(
                    f,
                    &transaction_with_meta.transaction.decode().unwrap(),
                    transaction_with_meta.meta.as_ref(),
                    "    ",
                    None,
                    None,
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedConfirmedBlockWithEntries {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub entries: Vec<CliPopulatedEntry>,
    pub rewards: Rewards,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
}

impl EncodedConfirmedBlockWithEntries {
    pub fn try_from(
        block: EncodedConfirmedBlock,
        entries_iterator: impl Iterator<Item = EntrySummary>,
    ) -> std::result::Result<Self, String> {
        let mut entries = vec![];
        for (i, entry) in entries_iterator.enumerate() {
            let ending_transaction_index = entry
                .starting_transaction_index
                .saturating_add(entry.num_transactions as usize);
            let transactions = block
                .transactions
                .get(entry.starting_transaction_index..ending_transaction_index)
                .ok_or(format!(
                    "Mismatched entry data and transactions: entry {:?}",
                    i
                ))?;
            entries.push(CliPopulatedEntry {
                num_hashes: entry.num_hashes,
                hash: entry.hash.to_string(),
                num_transactions: entry.num_transactions,
                starting_transaction_index: entry.starting_transaction_index,
                transactions: transactions.to_vec(),
            });
        }
        Ok(Self {
            previous_blockhash: block.previous_blockhash,
            blockhash: block.blockhash,
            parent_slot: block.parent_slot,
            entries,
            rewards: block.rewards,
            block_time: block.block_time,
            block_height: block.block_height,
        })
    }
}
