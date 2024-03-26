use {
    crate::instruction::CompiledInstruction,
    serde::{Deserialize, Serialize},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnerInstruction {
    pub instruction: CompiledInstruction,
    /// Invocation stack height of this instruction. Instruction stack height
    /// starts at 1 for transaction instructions.
    pub stack_height: u8,
}

/// An ordered list of compiled instructions that were invoked during a
/// transaction instruction
pub type InnerInstructions = Vec<InnerInstruction>;

/// A list of compiled instructions that were invoked during each instruction of
/// a transaction
pub type InnerInstructionsList = Vec<InnerInstructions>;
