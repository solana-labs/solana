use {
    super::core_bpf_migration::CoreBpfMigrationConfig,
    solana_program_runtime::invoke_context::BuiltinFunctionWithContext, solana_sdk::pubkey::Pubkey,
};

/// Transitions of built-in programs at epoch boundaries when features are activated.
pub struct BuiltinPrototype {
    pub(crate) core_bpf_migration_config: Option<CoreBpfMigrationConfig>,
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
        builder.field("core_bpf_migration_config", &self.core_bpf_migration_config);
        builder.finish()
    }
}

/// Transitions of stateless built-in programs at epoch boundaries when
/// features are activated.
/// These are built-in programs that don't actually exist, but their address
/// is reserved.
#[derive(Debug)]
pub struct StatelessBuiltinPrototype {
    pub(crate) core_bpf_migration_config: Option<CoreBpfMigrationConfig>,
    pub program_id: Pubkey,
    pub name: &'static str,
}
