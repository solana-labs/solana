use solana_sdk::instruction::CompiledInstruction;

/// A non-owning version of [`CompiledInstruction`] that references
/// slices of account indexes and data.
// `program_id_index` is still owned, as it is a simple u8.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SVMInstruction<'a> {
    /// Index into the transaction keys array indicating the program account that executes this instruction.
    pub program_id_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program.
    pub accounts: &'a [u8],
    /// The program input data.
    pub data: &'a [u8],
}

impl<'a> From<&'a CompiledInstruction> for SVMInstruction<'a> {
    fn from(ix: &'a CompiledInstruction) -> Self {
        Self {
            program_id_index: ix.program_id_index,
            accounts: ix.accounts.as_slice(),
            data: ix.data.as_slice(),
        }
    }
}
