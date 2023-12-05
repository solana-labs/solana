use {
    core::fmt,
    enum_iterator::Sequence,
    solana_sdk::{clock::Slot, pubkey::Pubkey, saturating_add_assign},
    std::{
        collections::HashMap,
        ops::{Index, IndexMut},
    },
};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ProgramTiming {
    pub accumulated_us: u64,
    pub accumulated_units: u64,
    pub count: u32,
    pub errored_txs_compute_consumed: Vec<u64>,
    // Sum of all units in `errored_txs_compute_consumed`
    pub total_errored_units: u64,
}

impl ProgramTiming {
    pub fn coalesce_error_timings(&mut self, current_estimated_program_cost: u64) {
        for tx_error_compute_consumed in self.errored_txs_compute_consumed.drain(..) {
            let compute_units_update =
                std::cmp::max(current_estimated_program_cost, tx_error_compute_consumed);
            saturating_add_assign!(self.accumulated_units, compute_units_update);
            saturating_add_assign!(self.count, 1);
        }
    }

    pub fn accumulate_program_timings(&mut self, other: &ProgramTiming) {
        saturating_add_assign!(self.accumulated_us, other.accumulated_us);
        saturating_add_assign!(self.accumulated_units, other.accumulated_units);
        saturating_add_assign!(self.count, other.count);
        // Clones the entire vector, maybe not great...
        self.errored_txs_compute_consumed
            .extend(other.errored_txs_compute_consumed.clone());
        saturating_add_assign!(self.total_errored_units, other.total_errored_units);
    }
}

/// Used as an index for `Metrics`.
#[derive(Debug, Sequence)]
pub enum ExecuteTimingType {
    CheckUs,
    LoadUs,
    ExecuteUs,
    StoreUs,
    UpdateStakesCacheUs,
    NumExecuteBatches,
    CollectLogsUs,
    TotalBatchesLen,
    UpdateTransactionStatuses,
}

pub struct Metrics([u64; ExecuteTimingType::CARDINALITY]);

impl Index<ExecuteTimingType> for Metrics {
    type Output = u64;
    fn index(&self, index: ExecuteTimingType) -> &Self::Output {
        self.0.index(index as usize)
    }
}

impl IndexMut<ExecuteTimingType> for Metrics {
    fn index_mut(&mut self, index: ExecuteTimingType) -> &mut Self::Output {
        self.0.index_mut(index as usize)
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics([0; ExecuteTimingType::CARDINALITY])
    }
}

impl core::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// The auxiliary variable that must always be provided to eager_macro_rules! must use the
// identifier `eager_1`. Macros declared with `eager_macro_rules!` can then be used inside
// an eager! block.
eager_macro_rules! { $eager_1
    #[macro_export]
    macro_rules! report_execute_timings {
        ($self: expr) => {
            (
                "validate_transactions_us",
                *$self
                    .metrics
                    .index(ExecuteTimingType::CheckUs),
                i64
            ),
            (
                "load_us",
                *$self
                    .metrics
                    .index(ExecuteTimingType::LoadUs),
                i64
            ),
            (
                "execute_us",
                *$self
                    .metrics
                    .index(ExecuteTimingType::ExecuteUs),
                i64
            ),
            (
                "collect_logs_us",
                *$self
                    .metrics
                    .index(ExecuteTimingType::CollectLogsUs),
                i64
            ),
            (
                "store_us",
                *$self

                    .metrics
                    .index(ExecuteTimingType::StoreUs),
                i64
            ),
            (
                "update_stakes_cache_us",
                *$self

                    .metrics
                    .index(ExecuteTimingType::UpdateStakesCacheUs),
                i64
            ),
            (
                "total_batches_len",
                *$self

                    .metrics
                    .index(ExecuteTimingType::TotalBatchesLen),
                i64
            ),
            (
                "num_execute_batches",
                *$self

                    .metrics
                    .index(ExecuteTimingType::NumExecuteBatches),
                i64
            ),
            (
                "update_transaction_statuses",
                *$self

                    .metrics
                    .index(ExecuteTimingType::UpdateTransactionStatuses),
                i64
            ),
            (
                "execute_details_serialize_us",
                $self.details.serialize_us,
                i64
            ),
            (
                "execute_details_create_vm_us",
                $self.details.create_vm_us,
                i64
            ),
            (
                "execute_details_execute_inner_us",
                $self.details.execute_us,
                i64
            ),
            (
                "execute_details_deserialize_us",
                $self.details.deserialize_us,
                i64
            ),
            (
                "execute_details_get_or_create_executor_us",
                $self.details.get_or_create_executor_us,
                i64
            ),
            (
                "execute_details_changed_account_count",
                $self.details.changed_account_count,
                i64
            ),
            (
                "execute_details_total_account_count",
                $self.details.total_account_count,
                i64
            ),
            (
                "execute_details_create_executor_register_syscalls_us",
                $self
                    .details
                    .create_executor_register_syscalls_us,
                i64
            ),
            (
                "execute_details_create_executor_load_elf_us",
                $self.details.create_executor_load_elf_us,
                i64
            ),
            (
                "execute_details_create_executor_verify_code_us",
                $self.details.create_executor_verify_code_us,
                i64
            ),
            (
                "execute_details_create_executor_jit_compile_us",
                $self.details.create_executor_jit_compile_us,
                i64
            ),
            (
                "execute_accessories_feature_set_clone_us",
                $self
                    .execute_accessories
                    .feature_set_clone_us,
                i64
            ),
            (
                "execute_accessories_compute_budget_process_transaction_us",
                $self
                    .execute_accessories
                    .compute_budget_process_transaction_us,
                i64
            ),
            (
                "execute_accessories_get_executors_us",
                $self.execute_accessories.get_executors_us,
                i64
            ),
            (
                "execute_accessories_process_message_us",
                $self.execute_accessories.process_message_us,
                i64
            ),
            (
                "execute_accessories_update_executors_us",
                $self.execute_accessories.update_executors_us,
                i64
            ),
            (
                "execute_accessories_process_instructions_total_us",
                $self
                    .execute_accessories
                    .process_instructions
                    .total_us,
                i64
            ),
            (
                "execute_accessories_process_instructions_verify_caller_us",
                $self
                    .execute_accessories
                    .process_instructions
                    .verify_caller_us,
                i64
            ),
            (
                "execute_accessories_process_instructions_process_executable_chain_us",
                $self
                    .execute_accessories
                    .process_instructions
                    .process_executable_chain_us,
                i64
            ),
            (
                "execute_accessories_process_instructions_verify_callee_us",
                $self
                    .execute_accessories
                    .process_instructions
                    .verify_callee_us,
                i64
            ),
        }
    }
}

#[derive(Debug, Default)]
pub struct ThreadExecuteTimings {
    pub total_thread_us: u64,
    pub total_transactions_executed: u64,
    pub execute_timings: ExecuteTimings,
}

impl ThreadExecuteTimings {
    pub fn report_stats(&self, slot: Slot) {
        lazy! {
            datapoint_info!(
                "replay-slot-end-to-end-stats",
                ("slot", slot as i64, i64),
                ("total_thread_us", self.total_thread_us as i64, i64),
                ("total_transactions_executed", self.total_transactions_executed as i64, i64),
                // Everything inside the `eager!` block will be eagerly expanded before
                // evaluation of the rest of the surrounding macro.
                eager!{report_execute_timings!(self.execute_timings)}
            );
        };
    }

    pub fn accumulate(&mut self, other: &ThreadExecuteTimings) {
        self.execute_timings.accumulate(&other.execute_timings);
        saturating_add_assign!(self.total_thread_us, other.total_thread_us);
        saturating_add_assign!(
            self.total_transactions_executed,
            other.total_transactions_executed
        );
    }
}

#[derive(Debug, Default)]
pub struct ExecuteTimings {
    pub metrics: Metrics,
    pub details: ExecuteDetailsTimings,
    pub execute_accessories: ExecuteAccessoryTimings,
}

impl ExecuteTimings {
    pub fn accumulate(&mut self, other: &ExecuteTimings) {
        for (t1, t2) in self.metrics.0.iter_mut().zip(other.metrics.0.iter()) {
            saturating_add_assign!(*t1, *t2);
        }
        self.details.accumulate(&other.details);
        self.execute_accessories
            .accumulate(&other.execute_accessories);
    }

    pub fn saturating_add_in_place(&mut self, timing_type: ExecuteTimingType, value_to_add: u64) {
        let idx = timing_type as usize;
        match self.metrics.0.get_mut(idx) {
            Some(elem) => *elem = elem.saturating_add(value_to_add),
            None => debug_assert!(idx < ExecuteTimingType::CARDINALITY, "Index out of bounds"),
        }
    }
}

#[derive(Default, Debug)]
pub struct ExecuteProcessInstructionTimings {
    pub total_us: u64,
    pub verify_caller_us: u64,
    pub process_executable_chain_us: u64,
    pub verify_callee_us: u64,
}

impl ExecuteProcessInstructionTimings {
    pub fn accumulate(&mut self, other: &ExecuteProcessInstructionTimings) {
        saturating_add_assign!(self.total_us, other.total_us);
        saturating_add_assign!(self.verify_caller_us, other.verify_caller_us);
        saturating_add_assign!(
            self.process_executable_chain_us,
            other.process_executable_chain_us
        );
        saturating_add_assign!(self.verify_callee_us, other.verify_callee_us);
    }
}

#[derive(Default, Debug)]
pub struct ExecuteAccessoryTimings {
    pub feature_set_clone_us: u64,
    pub compute_budget_process_transaction_us: u64,
    pub get_executors_us: u64,
    pub process_message_us: u64,
    pub update_executors_us: u64,
    pub process_instructions: ExecuteProcessInstructionTimings,
}

impl ExecuteAccessoryTimings {
    pub fn accumulate(&mut self, other: &ExecuteAccessoryTimings) {
        saturating_add_assign!(self.feature_set_clone_us, other.feature_set_clone_us);
        saturating_add_assign!(
            self.compute_budget_process_transaction_us,
            other.compute_budget_process_transaction_us
        );
        saturating_add_assign!(self.get_executors_us, other.get_executors_us);
        saturating_add_assign!(self.process_message_us, other.process_message_us);
        saturating_add_assign!(self.update_executors_us, other.update_executors_us);
        self.process_instructions
            .accumulate(&other.process_instructions);
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ExecuteDetailsTimings {
    pub serialize_us: u64,
    pub create_vm_us: u64,
    pub execute_us: u64,
    pub deserialize_us: u64,
    pub get_or_create_executor_us: u64,
    pub changed_account_count: u64,
    pub total_account_count: u64,
    pub create_executor_register_syscalls_us: u64,
    pub create_executor_load_elf_us: u64,
    pub create_executor_verify_code_us: u64,
    pub create_executor_jit_compile_us: u64,
    pub per_program_timings: HashMap<Pubkey, ProgramTiming>,
}

impl ExecuteDetailsTimings {
    pub fn accumulate(&mut self, other: &ExecuteDetailsTimings) {
        saturating_add_assign!(self.serialize_us, other.serialize_us);
        saturating_add_assign!(self.create_vm_us, other.create_vm_us);
        saturating_add_assign!(self.execute_us, other.execute_us);
        saturating_add_assign!(self.deserialize_us, other.deserialize_us);
        saturating_add_assign!(
            self.get_or_create_executor_us,
            other.get_or_create_executor_us
        );
        saturating_add_assign!(self.changed_account_count, other.changed_account_count);
        saturating_add_assign!(self.total_account_count, other.total_account_count);
        saturating_add_assign!(
            self.create_executor_register_syscalls_us,
            other.create_executor_register_syscalls_us
        );
        saturating_add_assign!(
            self.create_executor_load_elf_us,
            other.create_executor_load_elf_us
        );
        saturating_add_assign!(
            self.create_executor_verify_code_us,
            other.create_executor_verify_code_us
        );
        saturating_add_assign!(
            self.create_executor_jit_compile_us,
            other.create_executor_jit_compile_us
        );
        for (id, other) in &other.per_program_timings {
            let program_timing = self.per_program_timings.entry(*id).or_default();
            program_timing.accumulate_program_timings(other);
        }
    }

    pub fn accumulate_program(
        &mut self,
        program_id: &Pubkey,
        us: u64,
        compute_units_consumed: u64,
        is_error: bool,
    ) {
        let program_timing = self.per_program_timings.entry(*program_id).or_default();
        program_timing.accumulated_us = program_timing.accumulated_us.saturating_add(us);
        if is_error {
            program_timing
                .errored_txs_compute_consumed
                .push(compute_units_consumed);
            program_timing.total_errored_units = program_timing
                .total_errored_units
                .saturating_add(compute_units_consumed);
        } else {
            program_timing.accumulated_units = program_timing
                .accumulated_units
                .saturating_add(compute_units_consumed);
            program_timing.count = program_timing.count.saturating_add(1);
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn construct_execute_timings_with_program(
        program_id: &Pubkey,
        us: u64,
        compute_units_consumed: u64,
    ) -> ExecuteDetailsTimings {
        let mut execute_details_timings = ExecuteDetailsTimings::default();

        // Accumulate an erroring transaction
        let is_error = true;
        execute_details_timings.accumulate_program(
            program_id,
            us,
            compute_units_consumed,
            is_error,
        );

        // Accumulate a non-erroring transaction
        let is_error = false;
        execute_details_timings.accumulate_program(
            program_id,
            us,
            compute_units_consumed,
            is_error,
        );

        let program_timings = execute_details_timings
            .per_program_timings
            .get(program_id)
            .unwrap();

        // Both error and success transactions count towards `accumulated_us`
        assert_eq!(program_timings.accumulated_us, us.saturating_mul(2));
        assert_eq!(program_timings.accumulated_units, compute_units_consumed);
        assert_eq!(program_timings.count, 1,);
        assert_eq!(
            program_timings.errored_txs_compute_consumed,
            vec![compute_units_consumed]
        );
        assert_eq!(program_timings.total_errored_units, compute_units_consumed,);

        execute_details_timings
    }

    #[test]
    fn test_execute_details_timing_acumulate_program() {
        // Acumulate an erroring transaction
        let program_id = Pubkey::new_unique();
        let us = 100;
        let compute_units_consumed = 1;
        construct_execute_timings_with_program(&program_id, us, compute_units_consumed);
    }

    #[test]
    fn test_execute_details_timing_acumulate() {
        // Acumulate an erroring transaction
        let program_id = Pubkey::new_unique();
        let us = 100;
        let compute_units_consumed = 1;
        let mut execute_details_timings = ExecuteDetailsTimings::default();

        // Construct another separate instance of ExecuteDetailsTimings with non default fields
        let mut other_execute_details_timings =
            construct_execute_timings_with_program(&program_id, us, compute_units_consumed);
        let account_count = 1;
        other_execute_details_timings.serialize_us = us;
        other_execute_details_timings.create_vm_us = us;
        other_execute_details_timings.execute_us = us;
        other_execute_details_timings.deserialize_us = us;
        other_execute_details_timings.changed_account_count = account_count;
        other_execute_details_timings.total_account_count = account_count;

        // Accumulate the other instance into the current instance
        execute_details_timings.accumulate(&other_execute_details_timings);

        // Check that the two instances are equal
        assert_eq!(execute_details_timings, other_execute_details_timings);
    }

    #[test]
    fn execute_timings_saturating_add_in_place() {
        let mut timings = ExecuteTimings::default();
        timings.saturating_add_in_place(ExecuteTimingType::CheckUs, 1);
        let check_us = timings.metrics.index(ExecuteTimingType::CheckUs);
        assert_eq!(1, *check_us);

        timings.saturating_add_in_place(ExecuteTimingType::CheckUs, 2);
        let check_us = timings.metrics.index(ExecuteTimingType::CheckUs);
        assert_eq!(3, *check_us);
    }
}
