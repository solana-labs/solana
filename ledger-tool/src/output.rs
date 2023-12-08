use {
    serde::{Deserialize, Serialize},
    solana_cli_output::{QuietDisplay, VerboseDisplay},
    solana_sdk::clock::Slot,
    solana_transaction_status::EntrySummary,
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
            writeln!(
                f,
                "  Entry {} - num_hashes: {}, hash: {}, transactions: {}, starting_transaction_index: {}",
                i,
                entry.num_hashes,
                entry.hash,
                entry.num_transactions,
                entry.starting_transaction_index,
            )?;
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
