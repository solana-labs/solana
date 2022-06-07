//! this service receives instruction ExecuteTimings from replay_stage,
//! update cost_model which is shared with banking_stage to optimize
//! packing transactions into block; it also triggers persisting cost
//! table to blockstore.

use {
    crossbeam_channel::Receiver,
    solana_ledger::blockstore::Blockstore,
    solana_measure::measure,
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{bank::Bank, cost_model::CostModel},
    solana_sdk::timing::timestamp,
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
        self.update_cost_model_count += update_cost_model_count;
        self.update_cost_model_elapsed += update_cost_model_elapsed;

        let now = timestamp();
        let elapsed_ms = now - self.last_print;
        if elapsed_ms > 1000 {
            datapoint_info!(
                "cost-update-service-stats",
                ("total_elapsed_us", elapsed_ms * 1000, i64),
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
        for cost_update in cost_update_receiver.iter() {
            match cost_update {
                CostUpdate::FrozenBank { bank } => {
                    bank.read_cost_tracker().unwrap().report_stats(bank.slot());
                }
                CostUpdate::ExecuteTiming {
                    mut execute_timings,
                } => {
                    let (update_count, update_cost_model_time) = measure!(
                        Self::update_cost_model(&cost_model, &mut execute_timings),
                        "update_cost_model_time",
                    );
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
        }
        update_count
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_program_runtime::timings::ProgramTiming, solana_sdk::pubkey::Pubkey};

    #[test]
    fn test_update_cost_model_with_empty_execute_timings() {
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let mut empty_execute_timings = ExecuteTimings::default();

        assert_eq!(
            0,
            CostUpdateService::update_cost_model(&cost_model, &mut empty_execute_timings),
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
            assert_eq!(
                1,
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
            );
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
            assert_eq!(
                1,
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
            );
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
            assert_eq!(
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
                1
            );
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
        // the expect cost is (previous_cost + new_cost)/2 = (100 + 1000)/2 = 550
        let expected_units = 550;
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
            assert_eq!(
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
                1
            );
            assert_eq!(
                expected_units,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }

        // Test updating cost model with only erroring compute costs where the error cost is
        // `smaller_cost_per_error`, less than the current instruction cost for the program.
        // The cost should not decrease for these new lesser errors
        let smaller_cost_per_error = expected_units - 10;
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
            assert_eq!(
                CostUpdateService::update_cost_model(&cost_model, &mut execute_timings),
                1
            );
            assert_eq!(
                expected_units,
                cost_model
                    .read()
                    .unwrap()
                    .find_instruction_cost(&program_key_1)
            );
        }
    }
}
