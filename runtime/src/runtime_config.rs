use {solana_program_runtime::compute_budget::ComputeBudget, solana_sdk::clock::Slot};

/// Encapsulates flags that can be used to tweak the runtime behavior.
#[derive(Default, Clone)]
pub struct RuntimeConfig {
    pub bpf_jit: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub compute_budget: Option<ComputeBudget>,
}
