#![allow(dead_code)] // Removed in later commit
pub(crate) mod error;
mod source_upgradeable_bpf;
mod target_builtin;

pub(crate) enum CoreBpfMigrationTargetType {
    Builtin,
    Stateless,
}
