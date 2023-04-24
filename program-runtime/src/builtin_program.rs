#[cfg(RUSTC_WITH_SPECIALIZATION)]
use {crate::declare_process_instruction, solana_frozen_abi::abi_example::AbiExample};
use {
    crate::invoke_context::InvokeContext, solana_rbpf::vm::BuiltInFunction,
    solana_sdk::pubkey::Pubkey,
};

pub type ProcessInstructionWithContext = BuiltInFunction<InvokeContext<'static>>;

#[derive(Clone)]
pub struct BuiltinProgram {
    pub name: String,
    pub program_id: Pubkey,
    pub process_instruction: ProcessInstructionWithContext,
}

impl std::fmt::Debug for BuiltinProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Builtin [name={}, id={}]", self.name, self.program_id)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for BuiltinProgram {
    fn example() -> Self {
        declare_process_instruction!(empty_mock_process_instruction, 1, |_invoke_context| {
            // Do nothing
            Ok(())
        });

        Self {
            name: String::default(),
            program_id: Pubkey::default(),
            process_instruction: empty_mock_process_instruction,
        }
    }
}
