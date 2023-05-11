#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    crate::{invoke_context::InvokeContext, loaded_programs::LoadedProgram},
    solana_rbpf::vm::{BuiltInFunction, BuiltInProgram},
    solana_sdk::pubkey::Pubkey,
    std::sync::Arc,
};

pub type ProcessInstructionWithContext = BuiltInFunction<InvokeContext<'static>>;

pub fn create_builtin(
    name: String,
    process_instruction: ProcessInstructionWithContext,
) -> Arc<LoadedProgram> {
    let mut program = BuiltInProgram::default();
    program
        .register_function(b"entrypoint", process_instruction)
        .unwrap();
    Arc::new(LoadedProgram::new_builtin(name, 0, program))
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinPrograms {
    pub vec: Vec<(Pubkey, Arc<LoadedProgram>)>,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for BuiltinPrograms {
    fn example() -> Self {
        Self::default()
    }
}

impl BuiltinPrograms {
    pub fn new_mock(
        program_id: Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) -> Self {
        Self {
            vec: vec![(
                program_id,
                create_builtin("mockup".to_string(), process_instruction),
            )],
        }
    }
}
