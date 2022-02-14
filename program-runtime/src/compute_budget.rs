use solana_sdk::{
    borsh::try_from_slice_unchecked,
    compute_budget::{self, ComputeBudgetInstruction},
    entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
    instruction::InstructionError,
    message::SanitizedMessage,
    transaction::TransactionError,
};

const MAX_UNITS: u32 = 1_400_000;
const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for ComputeBudget {
    fn example() -> Self {
        // ComputeBudget is not Serialize so just rely on Default.
        ComputeBudget::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputeBudget {
    /// Number of compute units that an instruction is allowed.  Compute units
    /// are consumed by program execution, resources they use, etc...
    pub max_units: u64,
    /// Number of compute units consumed by a log_u64 call
    pub log_64_units: u64,
    /// Number of compute units consumed by a create_program_address call
    pub create_program_address_units: u64,
    /// Number of compute units consumed by an invoke call (not including the cost incurred by
    /// the called program)
    pub invoke_units: u64,
    /// Maximum cross-program invocation depth allowed
    pub max_invoke_depth: usize,
    /// Base number of compute units consumed to call SHA256
    pub sha256_base_cost: u64,
    /// Incremental number of units consumed by SHA256 (based on bytes)
    pub sha256_byte_cost: u64,
    /// Maximum BPF to BPF call depth
    pub max_call_depth: usize,
    /// Size of a stack frame in bytes, must match the size specified in the LLVM BPF backend
    pub stack_frame_size: usize,
    /// Number of compute units consumed by logging a `Pubkey`
    pub log_pubkey_units: u64,
    /// Maximum cross-program invocation instruction size
    pub max_cpi_instruction_size: usize,
    /// Number of account data bytes per conpute unit charged during a cross-program invocation
    pub cpi_bytes_per_unit: u64,
    /// Base number of compute units consumed to get a sysvar
    pub sysvar_base_cost: u64,
    /// Number of compute units consumed to call secp256k1_recover
    pub secp256k1_recover_cost: u64,
    /// Number of compute units consumed to do a syscall without any work
    pub syscall_base_cost: u64,
    /// Number of compute units consumed to call zktoken_crypto_op
    pub zk_token_elgamal_op_cost: u64,
    /// Optional program heap region size, if `None` then loader default
    pub heap_size: Option<usize>,
    /// Number of compute units per additional 32k heap above the default (~.5
    /// us per 32k at 15 units/us rounded up)
    pub heap_cost: u64,
    /// Memory operation syscall base cost
    pub mem_op_base_cost: u64,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ComputeBudget {
    pub fn new(use_max_units_default: bool) -> Self {
        let max_units = if use_max_units_default {
            MAX_UNITS
        } else {
            200_000
        } as u64;
        ComputeBudget {
            max_units,
            log_64_units: 100,
            create_program_address_units: 1500,
            invoke_units: 1000,
            max_invoke_depth: 4,
            sha256_base_cost: 85,
            sha256_byte_cost: 1,
            max_call_depth: 64,
            stack_frame_size: 4_096,
            log_pubkey_units: 100,
            max_cpi_instruction_size: 1280, // IPv6 Min MTU size
            cpi_bytes_per_unit: 250,        // ~50MB at 200,000 units
            sysvar_base_cost: 100,
            secp256k1_recover_cost: 25_000,
            syscall_base_cost: 100,
            zk_token_elgamal_op_cost: 25_000,
            heap_size: None,
            heap_cost: 8,
            mem_op_base_cost: 10,
        }
    }

    pub fn process_message(
        &mut self,
        message: &SanitizedMessage,
        requestable_heap_size: bool,
    ) -> Result<u64, TransactionError> {
        let mut requested_additional_fee = 0;
        let error = TransactionError::InstructionError(0, InstructionError::InvalidInstructionData);
        // Compute budget instruction must be in the 1st 3 instructions (avoid
        // nonce marker), otherwise ignored
        for (program_id, instruction) in message.program_instructions_iter().take(3) {
            if compute_budget::check_id(program_id) {
                match try_from_slice_unchecked(&instruction.data) {
                    Ok(ComputeBudgetInstruction::RequestUnits {
                        units,
                        additional_fee,
                    }) => {
                        self.max_units = units.min(MAX_UNITS) as u64;
                        requested_additional_fee = additional_fee as u64;
                    }
                    Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                        if !requestable_heap_size
                            || bytes > MAX_HEAP_FRAME_BYTES
                            || bytes < MIN_HEAP_FRAME_BYTES as u32
                            || bytes % 1024 != 0
                        {
                            return Err(error);
                        }
                        self.heap_size = Some(bytes as usize);
                    }
                    _ => return Err(error),
                }
            }
        }
        Ok(requested_additional_fee)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            instruction::Instruction,
            message::Message,
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    macro_rules! test {
        ( $instructions: expr, $expected_error: expr, $expected_budget: expr ) => {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            let mut compute_budget = ComputeBudget::default();
            let result = compute_budget.process_message(&tx.message(), true);
            assert_eq!($expected_error, result);
            assert_eq!(compute_budget, $expected_budget);
        };
    }

    #[test]
    fn test_process_mesage() {
        // Units
        test!(&[], Ok(0), ComputeBudget::default());
        test!(
            &[
                ComputeBudgetInstruction::request_units(1, 0),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(0),
            ComputeBudget {
                max_units: 1,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::request_units(MAX_UNITS + 1, 0),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(0),
            ComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_units(MAX_UNITS, 0),
            ],
            Ok(0),
            ComputeBudget {
                max_units: MAX_UNITS as u64,
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_units(1, 0),
            ],
            Ok(0),
            ComputeBudget::default()
        );

        // HeapFrame
        test!(&[], Ok(0), ComputeBudget::default());
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(0),
            ComputeBudget {
                heap_size: Some(40 * 1024),
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(31 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            ComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(0),
            ComputeBudget {
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(1), // ignored
            ],
            Ok(0),
            ComputeBudget::default()
        );

        // Combined
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::request_units(MAX_UNITS, 0),
            ],
            Ok(0),
            ComputeBudget {
                max_units: MAX_UNITS as u64,
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..ComputeBudget::default()
            }
        );
    }
}
