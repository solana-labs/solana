use {
    crate::invoke_context::InvokeContext, solana_frozen_abi::abi_example::AbiExample,
    solana_rbpf::vm::BuiltInFunction, solana_sdk::pubkey::Pubkey,
};

pub type ProcessInstructionWithContext = BuiltInFunction<InvokeContext<'static>>;

#[derive(Clone)]
pub struct BuiltinProgram {
    pub program_id: Pubkey,
    pub process_instruction: ProcessInstructionWithContext,
}

impl std::fmt::Debug for BuiltinProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}: {:p}",
            self.program_id, self.process_instruction as *const ProcessInstructionWithContext,
        )
    }
}
