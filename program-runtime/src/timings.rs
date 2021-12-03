use {solana_sdk::pubkey::Pubkey, std::collections::HashMap};

#[derive(Default, Debug)]
pub struct ProgramTiming {
    pub accumulated_us: u64,
    pub accumulated_units: u64,
    pub count: u32,
}

#[derive(Default, Debug)]
pub struct ExecuteDetailsTimings {
    pub serialize_us: u64,
    pub create_vm_us: u64,
    pub execute_us: u64,
    pub deserialize_us: u64,
    pub changed_account_count: u64,
    pub total_account_count: u64,
    pub total_data_size: usize,
    pub data_size_changed: usize,
    pub per_program_timings: HashMap<Pubkey, ProgramTiming>,
}
impl ExecuteDetailsTimings {
    pub fn accumulate(&mut self, other: &ExecuteDetailsTimings) {
        self.serialize_us = self.serialize_us.saturating_add(other.serialize_us);
        self.create_vm_us = self.create_vm_us.saturating_add(other.create_vm_us);
        self.execute_us = self.execute_us.saturating_add(other.execute_us);
        self.deserialize_us = self.deserialize_us.saturating_add(other.deserialize_us);
        self.changed_account_count = self
            .changed_account_count
            .saturating_add(other.changed_account_count);
        self.total_account_count = self
            .total_account_count
            .saturating_add(other.total_account_count);
        self.total_data_size = self.total_data_size.saturating_add(other.total_data_size);
        self.data_size_changed = self
            .data_size_changed
            .saturating_add(other.data_size_changed);
        for (id, other) in &other.per_program_timings {
            let program_timing = self.per_program_timings.entry(*id).or_default();
            program_timing.accumulated_us = program_timing
                .accumulated_us
                .saturating_add(other.accumulated_us);
            program_timing.accumulated_units = program_timing
                .accumulated_units
                .saturating_add(other.accumulated_units);
            program_timing.count = program_timing.count.saturating_add(other.count);
        }
    }
    pub fn accumulate_program(&mut self, program_id: &Pubkey, us: u64, units: u64) {
        let program_timing = self.per_program_timings.entry(*program_id).or_default();
        program_timing.accumulated_us = program_timing.accumulated_us.saturating_add(us);
        program_timing.accumulated_units = program_timing.accumulated_units.saturating_add(units);
        program_timing.count = program_timing.count.saturating_add(1);
    }
}
