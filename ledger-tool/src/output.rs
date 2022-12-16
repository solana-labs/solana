use {
    serde::Serialize,
    solana_cli_output::{QuietDisplay, VerboseDisplay},
    std::fmt::{Display, Formatter, Result},
};

#[derive(Serialize, Debug, Default)]
pub struct SlotInfo {
    pub start: u64,
    pub end: u64,
    pub total: usize,
    #[serde(rename = "fromLast", skip_serializing_if = "Option::is_none")]
    pub from_last: Option<u64>,
}

impl Display for SlotInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "Start: {}", &self.start)?;
        writeln!(f, "End: {}", &self.end)?;
        writeln!(f, "Total: {}", &self.total)?;
        if let Some(from_last) = &self.from_last {
            writeln!(f, "From Last: {}", from_last)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotBounds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_slots: Option<SlotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rooted_slots: Option<SlotInfo>,
}

impl VerboseDisplay for SlotBounds {}
impl QuietDisplay for SlotBounds {}

impl Display for SlotBounds {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if let Some(total_slots) = &self.total_slots {
            writeln!(f, "Total Slots: {}", total_slots)?;

            if let Some(rooted_slots) = &self.rooted_slots {
                writeln!(f, "Rooted Slots: {}", rooted_slots)?;
            }
        }
        Ok(())
    }
}
