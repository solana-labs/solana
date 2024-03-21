use {
    solana_program_runtime::invoke_context::BuiltinFunctionWithContext, solana_sdk::pubkey::Pubkey,
};

/// Transitions of built-in programs at epoch boundaries when features are activated.
pub struct BuiltinPrototype {
    pub enable_feature_id: Option<Pubkey>,
    pub program_id: Pubkey,
    pub name: &'static str,
    pub entrypoint: BuiltinFunctionWithContext,
}

impl std::fmt::Debug for BuiltinPrototype {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut builder = f.debug_struct("BuiltinPrototype");
        builder.field("program_id", &self.program_id);
        builder.field("name", &self.name);
        builder.field("enable_feature_id", &self.enable_feature_id);
        builder.finish()
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl solana_frozen_abi::abi_example::AbiExample for BuiltinPrototype {
    fn example() -> Self {
        // BuiltinPrototype isn't serializable by definition.
        solana_program_runtime::declare_process_instruction!(MockBuiltin, 0, |_invoke_context| {
            // Do nothing
            Ok(())
        });
        Self {
            enable_feature_id: None,
            program_id: Pubkey::default(),
            name: "",
            entrypoint: MockBuiltin::vm,
        }
    }
}

/// Transitions of stateless built-in programs at epoch boundaries when
/// features are activated.
/// These are built-in programs that don't actually exist, but their address
/// is reserved.
#[derive(Debug)]
pub struct StatelessBuiltinPrototype {
    pub program_id: Pubkey,
    pub name: &'static str,
}
