//! Process compute_budget instructions to extract and sanitize limits.
use {
    crate::{
        compute_budget::DEFAULT_HEAP_COST,
        prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType},
    },
    solana_sdk::{
        borsh0_10::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
        feature_set::{
            add_set_tx_loaded_accounts_data_size_instruction, remove_deprecated_request_unit_ix,
            FeatureSet,
        },
        fee::FeeBudgetLimits,
        instruction::{CompiledInstruction, InstructionError},
        pubkey::Pubkey,
        transaction::TransactionError,
    },
};

const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;
pub const DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// The total accounts data a transaction can load is limited to 64MiB to not break
/// anyone in Mainnet-beta today. It can be set by set_loaded_accounts_data_size_limit instruction
pub const MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES: u32 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeBudgetLimits {
    pub updated_heap_bytes: u32,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub loaded_accounts_bytes: u32,
    pub deprecated_additional_fee: Option<u64>,
}

impl Default for ComputeBudgetLimits {
    fn default() -> Self {
        ComputeBudgetLimits {
            updated_heap_bytes: u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap(),
            compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
            compute_unit_price: 0,
            loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
            deprecated_additional_fee: None,
        }
    }
}

impl From<ComputeBudgetLimits> for FeeBudgetLimits {
    fn from(val: ComputeBudgetLimits) -> Self {
        let prioritization_fee =
            if let Some(deprecated_additional_fee) = val.deprecated_additional_fee {
                deprecated_additional_fee
            } else {
                let prioritization_fee_details = PrioritizationFeeDetails::new(
                    PrioritizationFeeType::ComputeUnitPrice(val.compute_unit_price),
                    u64::from(val.compute_unit_limit),
                );
                prioritization_fee_details.get_fee()
            };

        FeeBudgetLimits {
            // NOTE - usize::from(u32).unwrap() may fail if target is 16-bit and
            // `loaded_accounts_bytes` is greater than u16::MAX. In that case, panic is proper.
            loaded_accounts_data_size_limit: usize::try_from(val.loaded_accounts_bytes).unwrap(),
            heap_cost: DEFAULT_HEAP_COST,
            compute_unit_limit: u64::from(val.compute_unit_limit),
            prioritization_fee,
        }
    }
}

/// Processing compute_budget could be part of tx sanitizing, failed to process
/// these instructions will drop the transaction eventually without execution,
/// may as well fail it early.
/// If succeeded, the transaction's specific limits/requests (could be default)
/// are retrieved and returned,
pub fn process_compute_budget_instructions<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
    feature_set: &FeatureSet,
) -> Result<ComputeBudgetLimits, TransactionError> {
    let support_request_units_deprecated =
        !feature_set.is_active(&remove_deprecated_request_unit_ix::id());
    let support_set_loaded_accounts_data_size_limit_ix =
        feature_set.is_active(&add_set_tx_loaded_accounts_data_size_instruction::id());

    let mut num_non_compute_budget_instructions: u32 = 0;
    let mut updated_compute_unit_limit = None;
    let mut updated_compute_unit_price = None;
    let mut requested_heap_size = None;
    let mut updated_loaded_accounts_data_size_limit = None;
    let mut deprecated_additional_fee = None;

    for (i, (program_id, instruction)) in instructions.enumerate() {
        if compute_budget::check_id(program_id) {
            let invalid_instruction_data_error = TransactionError::InstructionError(
                i as u8,
                InstructionError::InvalidInstructionData,
            );
            let duplicate_instruction_error = TransactionError::DuplicateInstruction(i as u8);

            match try_from_slice_unchecked(&instruction.data) {
                Ok(ComputeBudgetInstruction::RequestUnitsDeprecated {
                    units: compute_unit_limit,
                    additional_fee,
                }) if support_request_units_deprecated => {
                    if updated_compute_unit_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    if updated_compute_unit_price.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_limit = Some(compute_unit_limit);
                    updated_compute_unit_price =
                        support_deprecated_requested_units(additional_fee, compute_unit_limit);
                    deprecated_additional_fee = Some(u64::from(additional_fee));
                }
                Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                    if requested_heap_size.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    if sanitize_requested_heap_size(bytes) {
                        requested_heap_size = Some(bytes);
                    } else {
                        return Err(invalid_instruction_data_error);
                    }
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitLimit(compute_unit_limit)) => {
                    if updated_compute_unit_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_limit = Some(compute_unit_limit);
                }
                Ok(ComputeBudgetInstruction::SetComputeUnitPrice(micro_lamports)) => {
                    if updated_compute_unit_price.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_compute_unit_price = Some(micro_lamports);
                }
                Ok(ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit(bytes))
                    if support_set_loaded_accounts_data_size_limit_ix =>
                {
                    if updated_loaded_accounts_data_size_limit.is_some() {
                        return Err(duplicate_instruction_error);
                    }
                    updated_loaded_accounts_data_size_limit = Some(bytes);
                }
                _ => return Err(invalid_instruction_data_error),
            }
        } else {
            // only include non-request instructions in default max calc
            num_non_compute_budget_instructions =
                num_non_compute_budget_instructions.saturating_add(1);
        }
    }

    // sanitize limits
    let updated_heap_bytes = requested_heap_size
        .unwrap_or(u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap()) // loader's default heap_size
        .min(MAX_HEAP_FRAME_BYTES);

    let compute_unit_limit = updated_compute_unit_limit
        .unwrap_or_else(|| {
            num_non_compute_budget_instructions
                .saturating_mul(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)
        })
        .min(MAX_COMPUTE_UNIT_LIMIT);

    let compute_unit_price = updated_compute_unit_price.unwrap_or(0);

    let loaded_accounts_bytes = updated_loaded_accounts_data_size_limit
        .unwrap_or(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES)
        .min(MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES);

    Ok(ComputeBudgetLimits {
        updated_heap_bytes,
        compute_unit_limit,
        compute_unit_price,
        loaded_accounts_bytes,
        deprecated_additional_fee,
    })
}

fn sanitize_requested_heap_size(bytes: u32) -> bool {
    (u32::try_from(MIN_HEAP_FRAME_BYTES).unwrap()..=MAX_HEAP_FRAME_BYTES).contains(&bytes)
        && bytes % 1024 == 0
}

// Supports request_units_deprecated ix, returns compute_unit_price from deprecated requested
// units.
fn support_deprecated_requested_units(additional_fee: u32, compute_unit_limit: u32) -> Option<u64> {
    // TODO: remove support of 'Deprecated' after feature remove_deprecated_request_unit_ix::id() is activated
    let prioritization_fee_details = PrioritizationFeeDetails::new(
        PrioritizationFeeType::Deprecated(u64::from(additional_fee)),
        u64::from(compute_unit_limit),
    );
    Some(prioritization_fee_details.get_priority())
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
            system_instruction::{self},
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    macro_rules! test {
        ( $instructions: expr, $expected_result: expr, $support_set_loaded_accounts_data_size_limit_ix: expr ) => {
            let payer_keypair = Keypair::new();
            let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            ));
            let mut feature_set = FeatureSet::default();
            feature_set.activate(&remove_deprecated_request_unit_ix::id(), 0);
            if $support_set_loaded_accounts_data_size_limit_ix {
                feature_set.activate(&add_set_tx_loaded_accounts_data_size_instruction::id(), 0);
            }
            let result = process_compute_budget_instructions(
                tx.message().program_instructions_iter(),
                &feature_set,
            );
            assert_eq!($expected_result, result);
        };
        ( $instructions: expr, $expected_result: expr ) => {
            test!($instructions, $expected_result, false);
        };
    }

    #[test]
    fn test_process_instructions() {
        // Units
        test!(
            &[],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::set_compute_unit_price(42)
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 1,
                compute_unit_price: 42,
                ..ComputeBudgetLimits::default()
            })
        );

        // HeapFrame
        test!(
            &[],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 0,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: 40 * 1024,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(31 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(1),
            ],
            Err(TransactionError::InstructionError(
                3,
                InstructionError::InvalidInstructionData,
            ))
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT * 7,
                ..ComputeBudgetLimits::default()
            })
        );

        // Combined
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_price: u64::MAX,
                compute_unit_limit: MAX_COMPUTE_UNIT_LIMIT,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(1),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Ok(ComputeBudgetLimits {
                compute_unit_price: u64::MAX,
                compute_unit_limit: 1,
                updated_heap_bytes: MAX_HEAP_FRAME_BYTES,
                ..ComputeBudgetLimits::default()
            })
        );

        // Duplicates
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT),
                ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_UNIT_LIMIT - 1),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );

        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::request_heap_frame(MIN_HEAP_FRAME_BYTES as u32),
                ComputeBudgetInstruction::request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ComputeBudgetInstruction::set_compute_unit_price(0),
                ComputeBudgetInstruction::set_compute_unit_price(u64::MAX),
            ],
            Err(TransactionError::DuplicateInstruction(2))
        );

        // deprecated
        test!(
            &[Instruction::new_with_borsh(
                compute_budget::id(),
                &compute_budget::ComputeBudgetInstruction::RequestUnitsDeprecated {
                    units: 1_000,
                    additional_fee: 10
                },
                vec![]
            )],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            ))
        );
    }

    #[test]
    fn test_process_loaded_accounts_data_size_limit_instruction() {
        // Assert for empty instructions, change value of support_set_loaded_accounts_data_size_limit_ix
        // will not change results, which should all be default
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            test!(
                &[],
                Ok(ComputeBudgetLimits {
                    compute_unit_limit: 0,
                    ..ComputeBudgetLimits::default()
                }),
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents,
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set with data_size
        // else
        //     return InstructionError
        let data_size = 1;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let expected_result = if support_set_loaded_accounts_data_size_limit_ix {
                Ok(ComputeBudgetLimits {
                    compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                    loaded_accounts_bytes: data_size,
                    ..ComputeBudgetLimits::default()
                })
            } else {
                Err(TransactionError::InstructionError(
                    0,
                    InstructionError::InvalidInstructionData,
                ))
            };

            test!(
                &[
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ],
                expected_result,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents, with greater than max value
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set to max data size
        // else
        //     return InstructionError
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES + 1;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let expected_result = if support_set_loaded_accounts_data_size_limit_ix {
                Ok(ComputeBudgetLimits {
                    compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                    loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
                    ..ComputeBudgetLimits::default()
                })
            } else {
                Err(TransactionError::InstructionError(
                    0,
                    InstructionError::InvalidInstructionData,
                ))
            };

            test!(
                &[
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                ],
                expected_result,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit is not presented
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     budget is set to default data size
        // else
        //     return
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let expected_result = Ok(ComputeBudgetLimits {
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                loaded_accounts_bytes: MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES,
                ..ComputeBudgetLimits::default()
            });

            test!(
                &[Instruction::new_with_bincode(
                    Pubkey::new_unique(),
                    &0_u8,
                    vec![]
                ),],
                expected_result,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }

        // Assert when set_loaded_accounts_data_size_limit presents more than once,
        // if support_set_loaded_accounts_data_size_limit_ix then
        //     return DuplicateInstruction
        // else
        //     return InstructionError
        let data_size = MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES;
        for support_set_loaded_accounts_data_size_limit_ix in [true, false] {
            let expected_result = if support_set_loaded_accounts_data_size_limit_ix {
                Err(TransactionError::DuplicateInstruction(2))
            } else {
                Err(TransactionError::InstructionError(
                    1,
                    InstructionError::InvalidInstructionData,
                ))
            };

            test!(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size),
                ],
                expected_result,
                support_set_loaded_accounts_data_size_limit_ix
            );
        }
    }

    #[test]
    fn test_process_mixed_instructions_without_compute_budget() {
        let payer_keypair = Keypair::new();

        let transaction =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                    system_instruction::transfer(&payer_keypair.pubkey(), &Pubkey::new_unique(), 2),
                ],
                Some(&payer_keypair.pubkey()),
                &[&payer_keypair],
                Hash::default(),
            ));

        let mut feature_set = FeatureSet::default();
        feature_set.activate(&remove_deprecated_request_unit_ix::id(), 0);
        feature_set.activate(&add_set_tx_loaded_accounts_data_size_instruction::id(), 0);

        let result = process_compute_budget_instructions(
            transaction.message().program_instructions_iter(),
            &feature_set,
        );

        // assert process_instructions will be successful with default,
        // and the default compute_unit_limit is 2 times default: one for bpf ix, one for
        // builtin ix.
        assert_eq!(
            result,
            Ok(ComputeBudgetLimits {
                compute_unit_limit: 2 * DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                ..ComputeBudgetLimits::default()
            })
        );
    }

    fn try_prioritization_fee_from_deprecated_requested_units(
        additional_fee: u32,
        compute_unit_limit: u32,
    ) {
        let payer_keypair = Keypair::new();
        let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[Instruction::new_with_borsh(
                    compute_budget::id(),
                    &compute_budget::ComputeBudgetInstruction::RequestUnitsDeprecated {
                        units: compute_unit_limit,
                        additional_fee,
                    },
                    vec![],
                )],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        ));

        // sucessfully process deprecated instruction
        let compute_budget_limits = process_compute_budget_instructions(
            tx.message().program_instructions_iter(),
            &FeatureSet::default(),
        )
        .unwrap();

        // assert compute_budget_limit
        let expected_compute_unit_price = (additional_fee as u128)
            .saturating_mul(1_000_000)
            .checked_div(compute_unit_limit as u128)
            .map(|cu_price| u64::try_from(cu_price).unwrap_or(u64::MAX))
            .unwrap();
        let expected_compute_unit_limit = compute_unit_limit.min(MAX_COMPUTE_UNIT_LIMIT);
        assert_eq!(
            compute_budget_limits.compute_unit_price,
            expected_compute_unit_price
        );
        assert_eq!(
            compute_budget_limits.compute_unit_limit,
            expected_compute_unit_limit
        );

        // assert fee_budget_limits
        let fee_budget_limits = FeeBudgetLimits::from(compute_budget_limits);
        assert_eq!(
            fee_budget_limits.prioritization_fee,
            u64::from(additional_fee)
        );
        assert_eq!(
            fee_budget_limits.compute_unit_limit,
            u64::from(expected_compute_unit_limit)
        );
    }

    #[test]
    fn test_support_deprecated_requested_units() {
        // a normal case
        try_prioritization_fee_from_deprecated_requested_units(647, 6002);

        // requesting cu limit more than MAX, div result will be round down
        try_prioritization_fee_from_deprecated_requested_units(
            640,
            MAX_COMPUTE_UNIT_LIMIT + 606_002,
        );

        // requesting cu limit more than MAX, div result will round up
        try_prioritization_fee_from_deprecated_requested_units(
            764,
            MAX_COMPUTE_UNIT_LIMIT + 606_004,
        );
    }
}
