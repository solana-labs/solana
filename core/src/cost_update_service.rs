//! this service receives instruction ExecuteTimings from replay_stage,
//! update cost_model which is shared with banking_stage to optimize
//! packing transactions into block; it also triggers persisting cost
//! table to blockstore.

use {
    crossbeam_channel::Receiver,
    solana_ledger::blockstore::Blockstore,
    solana_measure::measure::Measure,
    solana_program_runtime::timings::{ExecuteTimings, ProgramTiming},
    solana_runtime::{bank::Bank, cost_model::CostModel},
    solana_sdk::{pubkey::Pubkey, timing::timestamp},
    std::{
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
};

#[derive(Default)]
pub struct CostUpdateServiceTiming {
    last_print: u64,
    update_cost_model_count: u64,
    update_cost_model_elapsed: u64,
}

impl CostUpdateServiceTiming {
    fn update(&mut self, update_cost_model_count: u64, update_cost_model_elapsed: u64) {
        self.update_cost_model_count = self
            .update_cost_model_count
            .saturating_add(update_cost_model_count);
        self.update_cost_model_elapsed = self
            .update_cost_model_elapsed
            .saturating_add(update_cost_model_elapsed);

        let now = timestamp();
        let elapsed_ms = now - self.last_print;
        if elapsed_ms > 1000 {
            datapoint_info!(
                "cost-update-service-stats",
                (
                    "update_cost_model_count",
                    self.update_cost_model_count as i64,
                    i64
                ),
                (
                    "update_cost_model_elapsed",
                    self.update_cost_model_elapsed as i64,
                    i64
                ),
            );

            *self = CostUpdateServiceTiming::default();
            self.last_print = now;
        }
    }
}

pub enum CostUpdate {
    FrozenBank {
        bank: Arc<Bank>,
    },
    ExecuteTiming {
        execute_timings: Box<ExecuteTimings>,
    },
}

pub type CostUpdateReceiver = Receiver<CostUpdate>;

pub struct CostUpdateService {
    thread_hdl: JoinHandle<()>,
}

impl CostUpdateService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        blockstore: Arc<Blockstore>,
        cost_model: Arc<RwLock<CostModel>>,
        cost_update_receiver: CostUpdateReceiver,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solana-cost-update-service".to_string())
            .spawn(move || {
                Self::service_loop(blockstore, cost_model, cost_update_receiver);
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }

    fn service_loop(
        _blockstore: Arc<Blockstore>,
        cost_model: Arc<RwLock<CostModel>>,
        cost_update_receiver: CostUpdateReceiver,
    ) {
        let mut cost_update_service_timing = CostUpdateServiceTiming::default();
        let mut update_count = 0_u64;

        for cost_update in cost_update_receiver.iter() {
            match cost_update {
                CostUpdate::FrozenBank { bank } => {
                    bank.read_cost_tracker().unwrap().report_stats(bank.slot());
                }
                CostUpdate::ExecuteTiming {
                    mut execute_timings,
                } => {
                    let mut update_cost_model_time = Measure::start("update_cost_model_time");
                    update_count += Self::update_cost_model(&cost_model, &mut execute_timings);
                    update_cost_model_time.stop();
                    cost_update_service_timing.update(update_count, update_cost_model_time.as_us());
                }
            }
        }
    }

    fn update_cost_model(
        cost_model: &RwLock<CostModel>,
        execute_timings: &mut ExecuteTimings,
    ) -> u64 {
        let mut update_count = 0_u64;
        for (program_id, program_timings) in &mut execute_timings.details.per_program_timings {
            let current_estimated_program_cost =
                cost_model.read().unwrap().find_instruction_cost(program_id);
            program_timings.coalesce_error_timings(current_estimated_program_cost);

            if program_timings.count < 1 {
                continue;
            }

            let units = program_timings.accumulated_units / program_timings.count as u64;
            cost_model
                .write()
                .unwrap()
                .upsert_instruction_cost(program_id, units);
            update_count += 1;

            let updated_estimated_program_cost =
                cost_model.read().unwrap().find_instruction_cost(program_id);
            if Self::is_large_change_in_cost_calculation(
                units as i64,
                updated_estimated_program_cost as i64,
            ) {
                Self::report_large_change_in_cost(
                    program_id,
                    program_timings,
                    updated_estimated_program_cost,
                );
            }
        }
        update_count
    }

    // Using historical data, it seems reasonable to consider large change if updated_cost
    // is more than double of input_data;
    // Compare updated_cost against input_cost instead of previous calculated cost captures
    // how far apart between single data point and calculated value, which is a good indicator
    // of potential issues.
    fn is_large_change_in_cost_calculation(input_cost: i64, updated_cost: i64) -> bool {
        (updated_cost - input_cost).abs() > input_cost
    }

    fn report_large_change_in_cost(
        program_id: &Pubkey,
        program_timing: &ProgramTiming,
        calculated_units: u64,
    ) {
        datapoint_info!(
            "large_change_in_cost",
            ("pubkey", program_id.to_string(), String),
            ("execute_us", program_timing.accumulated_us, i64),
            ("accumulated_units", program_timing.accumulated_units, i64),
            ("count", program_timing.count, i64),
            ("errored_units", program_timing.total_errored_units, i64),
            (
                "errored_count",
                program_timing.errored_txs_compute_consumed.len(),
                i64
            ),
            ("calculated_units", calculated_units as i64, i64),
        );
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_program_runtime::timings::ProgramTiming};

    #[test]
    fn test_update_cost_model_with_empty_execute_timings() {
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let mut empty_execute_timings = ExecuteTimings::default();
        assert_eq!(
            CostUpdateService::update_cost_model(&cost_model, &mut empty_execute_timings),
            0
        );
    }

    #[test]
    fn test_update_cost_model_with_execute_timings() {
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let mut execute_timings = ExecuteTimings::default();

        let program_key_1 = Pubkey::new_unique();
        let mut expected_cost: u64;

        // add new program
        {
            let accumulated_us: u64 = 1000;
            let accumulated_units: u64 = 100;
            let total_errored_units = 0;
            let count: u32 = 10;
            expected_cost = accumulated_units / count as u64;

            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us,
                    accumulated_units,
                    count,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units,
                },
            );
            let update_count =
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings);
            assert_eq!(1, update_count);
            assert_eq!(
                expected_cost,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }

        // update program
        {
            let accumulated_us: u64 = 2000;
            let accumulated_units: u64 = 200;
            let count: u32 = 10;
            // to expect new cost is Average(new_value, existing_value)
            expected_cost = ((accumulated_units / count as u64) + expected_cost) / 2;

            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us,
                    accumulated_units,
                    count,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units: 0,
                },
            );
            let update_count =
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings);
            assert_eq!(1, update_count);
            assert_eq!(
                expected_cost,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }
    }

    #[test]
    fn test_update_cost_model_with_error_execute_timings() {
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let mut execute_timings = ExecuteTimings::default();
        let program_key_1 = Pubkey::new_unique();

        // Test updating cost model with a `ProgramTiming` with no compute units accumulated, i.e.
        // `accumulated_units` == 0
        {
            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us: 1000,
                    accumulated_units: 0,
                    count: 0,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units: 0,
                },
            );
            // If both the `errored_txs_compute_consumed` is empty and `count == 0`, then
            // nothing should be inserted into the cost model
            assert_eq!(
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
                0
            );
        }

        // set up current instruction cost to 100
        let current_program_cost = 100;
        {
            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us: 1000,
                    accumulated_units: current_program_cost,
                    count: 1,
                    errored_txs_compute_consumed: vec![],
                    total_errored_units: 0,
                },
            );
            let update_count =
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings);
            assert_eq!(1, update_count);
            assert_eq!(
                current_program_cost,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }

        // Test updating cost model with only erroring compute costs where the `cost_per_error` is
        // greater than the current instruction cost for the program. Should update with the
        // new erroring compute costs
        let cost_per_error = 1000;
        {
            let errored_txs_compute_consumed = vec![cost_per_error; 3];
            let total_errored_units = errored_txs_compute_consumed.iter().sum();
            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us: 1000,
                    accumulated_units: 0,
                    count: 0,
                    errored_txs_compute_consumed,
                    total_errored_units,
                },
            );
            let update_count =
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings);
            assert_eq!(1, update_count);
            assert_eq!(
                cost_per_error,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }

        // Test updating cost model with only erroring compute costs where the error cost is
        // `smaller_cost_per_error`, less than the current instruction cost for the program.
        // The cost should not decrease for these new lesser errors
        let smaller_cost_per_error = cost_per_error - 10;
        {
            let errored_txs_compute_consumed = vec![smaller_cost_per_error; 3];
            let total_errored_units = errored_txs_compute_consumed.iter().sum();
            execute_timings.details.per_program_timings.insert(
                program_key_1,
                ProgramTiming {
                    accumulated_us: 1000,
                    accumulated_units: 0,
                    count: 0,
                    errored_txs_compute_consumed,
                    total_errored_units,
                },
            );
            let update_count =
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings);
            assert_eq!(1, update_count);
            assert_eq!(
                cost_per_error,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }
    }
}
