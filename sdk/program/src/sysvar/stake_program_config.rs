//! Sysvar for the stake program's configuration parameters
use crate::{
    impl_sysvar_get,
    sysvar::{ProgramError, Sysvar},
};

crate::declare_sysvar_id!(
    "SysvarStakeProgramConfig1111111111111111111",
    StakeProgramConfig
);

type Lamports = u64;

// bprumo TODO: should I add a frozen abi hash here?
/// Configuration parameters for the stake program.
#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct StakeProgramConfig {
    /// The minimum stake delegation amount, in lamports
    minimum_delegation: Lamports,
}

impl StakeProgramConfig {
    /// Create a new StakeProgramConfig with `minimum_delegation`
    #[must_use]
    pub fn new(minimum_delegation: Lamports) -> Self {
        Self { minimum_delegation }
    }

    /// Get the minimum stake delegation amount, in lamports
    pub fn minimum_delegation(&self) -> Lamports {
        self.minimum_delegation
    }
}

impl Sysvar for StakeProgramConfig {
    impl_sysvar_get!(sol_get_stake_program_config_sysvar);
}
