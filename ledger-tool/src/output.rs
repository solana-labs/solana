use {
    serde::Serialize,
    solana_cli_output::{QuietDisplay, VerboseDisplay},
    std::fmt::{Display, Formatter, Result},
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

impl Display for SlotInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "Total: {}", &self.total)?;
        if let Some(first) = &self.first {
            writeln!(f, "First: {}", first)?;
        }
        if let Some(last) = &self.last {
            writeln!(f, "Last: {}", last)?;
        }
        if let Some(num_after_last_root) = &self.num_after_last_root {
            writeln!(f, "From Last: {}", num_after_last_root)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotBounds<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_slots: Option<&'a Vec<u64>>,
    pub slots: SlotInfo,
    pub roots: Option<SlotInfo>,
}

impl VerboseDisplay for SlotBounds<'_> {}
impl QuietDisplay for SlotBounds<'_> {}

impl Display for SlotBounds<'_> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if &self.slots.first != &self.slots.last {
            writeln!(
                f,
                "Ledger has data for {:?} slots {:?} to {:?}",
                &self.slots.total, &self.slots.first, &self.slots.last
            )?;

            if let Some(all_slots) = &self.all_slots {
                writeln!(f, "Non-empty slots: {:?}", all_slots)?;
            }
        } else {
            writeln!(f, "Ledger has data for slot {:?}", &self.slots.first)?;
        }

        if let Some(rooted) = &self.roots {
            writeln!(
                f,
                "  with {:?} rooted slots from {:?} to {:?}",
                rooted.total, rooted.first, rooted.last
            )?;

            if let Some(num_after_last_root) = rooted.num_after_last_root {
                writeln!(
                    f,
                    "  and {:?} slots past the last root",
                    num_after_last_root
                )?;
            }
        } else {
            writeln!(f, "  with no rooted slots")?;
        }

        Ok(())
    }
}
