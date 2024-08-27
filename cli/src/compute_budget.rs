use {
    solana_clap_utils::compute_budget::ComputeUnitLimit,
    solana_compute_budget::compute_budget_limits::MAX_COMPUTE_UNIT_LIMIT,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::config::RpcSimulateTransactionConfig,
    solana_sdk::{
        borsh1::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        instruction::Instruction,
        message::Message,
        transaction::Transaction,
    },
};

// This enum is equivalent to an Option but was added to self-document
// the ok variants and has the benefit of not forcing the caller to use
// the result if they don't care about it.
pub(crate) enum UpdateComputeUnitLimitResult {
    UpdatedInstructionIndex(usize),
    NoInstructionFound,
}

fn get_compute_unit_limit_instruction_index(message: &Message) -> Option<usize> {
    message
        .instructions
        .iter()
        .enumerate()
        .find_map(|(ix_index, instruction)| {
            let ix_program_id = message.program_id(ix_index)?;
            if ix_program_id != &compute_budget::id() {
                return None;
            }

            matches!(
                try_from_slice_unchecked(&instruction.data),
                Ok(ComputeBudgetInstruction::SetComputeUnitLimit(_))
            )
            .then_some(ix_index)
        })
}

/// Like `simulate_for_compute_unit_limit`, but does not check that the message
/// contains a compute unit limit instruction.
fn simulate_for_compute_unit_limit_unchecked(
    rpc_client: &RpcClient,
    message: &Message,
) -> Result<u32, Box<dyn std::error::Error>> {
    let transaction = Transaction::new_unsigned(message.clone());
    let simulate_result = rpc_client
        .simulate_transaction_with_config(
            &transaction,
            RpcSimulateTransactionConfig {
                replace_recent_blockhash: true,
                commitment: Some(rpc_client.commitment()),
                ..RpcSimulateTransactionConfig::default()
            },
        )?
        .value;

    // Bail if the simulated transaction failed
    if let Some(err) = simulate_result.err {
        return Err(err.into());
    }

    let units_consumed = simulate_result
        .units_consumed
        .expect("compute units unavailable");

    u32::try_from(units_consumed).map_err(Into::into)
}

/// Returns the compute unit limit used during simulation
///
/// Returns an error if the message does not contain a compute unit limit
/// instruction or if the simulation fails.
pub(crate) fn simulate_for_compute_unit_limit(
    rpc_client: &RpcClient,
    message: &Message,
) -> Result<u32, Box<dyn std::error::Error>> {
    if get_compute_unit_limit_instruction_index(message).is_none() {
        return Err("No compute unit limit instruction found".into());
    }
    simulate_for_compute_unit_limit_unchecked(rpc_client, message)
}

// Returns the index of the compute unit limit instruction
pub(crate) fn simulate_and_update_compute_unit_limit(
    rpc_client: &RpcClient,
    message: &mut Message,
) -> Result<UpdateComputeUnitLimitResult, Box<dyn std::error::Error>> {
    let Some(compute_unit_limit_ix_index) = get_compute_unit_limit_instruction_index(message)
    else {
        return Ok(UpdateComputeUnitLimitResult::NoInstructionFound);
    };

    let compute_unit_limit = simulate_for_compute_unit_limit_unchecked(rpc_client, message)?;

    // Overwrite the compute unit limit instruction with the actual units consumed
    message.instructions[compute_unit_limit_ix_index].data =
        ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit).data;

    Ok(UpdateComputeUnitLimitResult::UpdatedInstructionIndex(
        compute_unit_limit_ix_index,
    ))
}

pub(crate) struct ComputeUnitConfig {
    pub(crate) compute_unit_price: Option<u64>,
    pub(crate) compute_unit_limit: ComputeUnitLimit,
}

pub(crate) trait WithComputeUnitConfig {
    fn with_compute_unit_config(self, config: &ComputeUnitConfig) -> Self;
}

impl WithComputeUnitConfig for Vec<Instruction> {
    fn with_compute_unit_config(mut self, config: &ComputeUnitConfig) -> Self {
        if let Some(compute_unit_price) = config.compute_unit_price {
            self.push(ComputeBudgetInstruction::set_compute_unit_price(
                compute_unit_price,
            ));
            match config.compute_unit_limit {
                ComputeUnitLimit::Default => {}
                ComputeUnitLimit::Static(compute_unit_limit) => {
                    self.push(ComputeBudgetInstruction::set_compute_unit_limit(
                        compute_unit_limit,
                    ));
                }
                ComputeUnitLimit::Simulated => {
                    // Default to the max compute unit limit because later transactions will be
                    // simulated to get the exact compute units consumed.
                    self.push(ComputeBudgetInstruction::set_compute_unit_limit(
                        MAX_COMPUTE_UNIT_LIMIT,
                    ));
                }
            }
        }
        self
    }
}
