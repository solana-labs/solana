#[allow(deprecated)]
use solana_sdk::sysvar::fees::Fees;
use {
    solana_sdk::{
        instruction::InstructionError,
        sysvar::{
            clock::Clock, epoch_schedule::EpochSchedule, rent::Rent, slot_hashes::SlotHashes,
        },
    },
    std::sync::Arc,
};

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for SysvarCache {
    fn example() -> Self {
        // SysvarCache is not Serialize so just rely on Default.
        SysvarCache::default()
    }
}

#[derive(Default, Clone, Debug)]
pub struct SysvarCache {
    clock: Option<Arc<Clock>>,
    epoch_schedule: Option<Arc<EpochSchedule>>,
    #[allow(deprecated)]
    fees: Option<Arc<Fees>>,
    rent: Option<Arc<Rent>>,
    slot_hashes: Option<Arc<SlotHashes>>,
}

impl SysvarCache {
    pub fn get_clock(&self) -> Result<Arc<Clock>, InstructionError> {
        self.clock
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = Some(Arc::new(clock));
    }

    pub fn get_epoch_schedule(&self) -> Result<Arc<EpochSchedule>, InstructionError> {
        self.epoch_schedule
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_epoch_schedule(&mut self, epoch_schedule: EpochSchedule) {
        self.epoch_schedule = Some(Arc::new(epoch_schedule));
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn get_fees(&self) -> Result<Arc<Fees>, InstructionError> {
        self.fees.clone().ok_or(InstructionError::UnsupportedSysvar)
    }

    #[deprecated]
    #[allow(deprecated)]
    pub fn set_fees(&mut self, fees: Fees) {
        self.fees = Some(Arc::new(fees));
    }

    pub fn get_rent(&self) -> Result<Arc<Rent>, InstructionError> {
        self.rent.clone().ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_rent(&mut self, rent: Rent) {
        self.rent = Some(Arc::new(rent));
    }

    pub fn get_slot_hashes(&self) -> Result<Arc<SlotHashes>, InstructionError> {
        self.slot_hashes
            .clone()
            .ok_or(InstructionError::UnsupportedSysvar)
    }

    pub fn set_slot_hashes(&mut self, slot_hashes: SlotHashes) {
        self.slot_hashes = Some(Arc::new(slot_hashes));
    }
}
