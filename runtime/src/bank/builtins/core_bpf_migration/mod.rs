use solana_sdk::pubkey::Pubkey;

/// Configurations for migrating a built-in program to Core BPF.
pub struct CoreBpfMigrationConfig {
    /// The source program ID to replace the builtin with.
    pub source_program_id: Pubkey,
    /// The feature gate to trigger the migration to Core BPF.
    /// Note: This feature gate should never be the same as any builtin's
    /// `enable_feature_id`. It should always be a feature gate that will be
    /// activated after the builtin is already enabled.
    pub feature_id: Pubkey,
}

impl std::fmt::Debug for CoreBpfMigrationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut builder = f.debug_struct("CoreBpfMigrationConfig");
        builder.field("source_program_id", &self.source_program_id);
        builder.field("feature_id", &self.feature_id);
        builder.finish()
    }
}
