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
    pub num_after_last_root: Option<usize>
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
pub struct SlotBounds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slots: Option<SlotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<SlotInfo>,
}

impl VerboseDisplay for SlotBounds {}
impl QuietDisplay for SlotBounds {}

impl Display for SlotBounds {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if let Some(slots) = &self.slots {
            writeln!(f, "Total Slots: {}", slots)?;

            if let Some(roots) = &self.roots {
                writeln!(f, "Rooted Slots: {}", roots)?;
            }
        }
        Ok(())
    }
}
