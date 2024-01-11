mod bpf_program;
mod bpf_upgradeable_program;
pub(crate) mod error;
mod native_program;

use solana_sdk::pubkey::Pubkey;

/// Enum representing the native programs that can be migrated to BPF
/// programs
#[allow(dead_code)] // Code is off the hot path until a migration is due
#[derive(Clone, Copy)]
pub(crate) enum NativeProgram {
    AddressLookupTable,
    BpfLoader,
    BpfLoaderUpgradeable,
    ComputeBudget,
    Config,
    Ed25519,
    FeatureGate,
    LoaderV4,
    NativeLoader,
    Secp256k1,
    Stake,
    System,
    Vote,
    ZkTokenProof,
}
#[allow(dead_code)] // Code is off the hot path until a migration is due
impl NativeProgram {
    /// The program ID of the native program
    fn id(&self) -> Pubkey {
        match self {
            Self::AddressLookupTable => solana_sdk::address_lookup_table::program::id(),
            Self::BpfLoader => solana_sdk::bpf_loader::id(),
            Self::BpfLoaderUpgradeable => solana_sdk::bpf_loader_upgradeable::id(),
            Self::ComputeBudget => solana_sdk::compute_budget::id(),
            Self::Config => solana_sdk::config::program::id(),
            Self::Ed25519 => solana_sdk::ed25519_program::id(),
            Self::FeatureGate => solana_sdk::feature::id(),
            Self::LoaderV4 => solana_sdk::loader_v4::id(),
            Self::NativeLoader => solana_sdk::native_loader::id(),
            Self::Secp256k1 => solana_sdk::secp256k1_program::id(),
            Self::Stake => solana_sdk::stake::program::id(),
            Self::System => solana_sdk::system_program::id(),
            Self::Vote => solana_sdk::vote::program::id(),
            Self::ZkTokenProof => solana_zk_token_sdk::zk_token_proof_program::id(),
        }
    }

    /// Returns whether or not a native program's program account is synthetic,
    /// meaning it does not actually exist, rather its address is used as an
    /// owner for other accounts
    fn is_synthetic(&self) -> bool {
        match self {
            Self::FeatureGate
            | Self::LoaderV4 // Not deployed
            | Self::NativeLoader
            | Self::ZkTokenProof // Not deployed
            => true,
            _ => false,
        }
    }
}
