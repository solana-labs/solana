// static account keys has max
use {
    agave_transaction_view::static_account_keys_frame::MAX_STATIC_ACCOUNTS_PER_PACKET as FILTER_SIZE,
    solana_pubkey::Pubkey, solana_svm_transaction::instruction::SVMInstruction,
};

pub struct PrecompileSignatureDetails {
    pub num_secp256k1_instruction_signatures: u64,
    pub num_ed25519_instruction_signatures: u64,
}

/// Get transaction signature details.
pub fn get_precompile_signature_details<'a>(
    instructions: impl Iterator<Item = (&'a Pubkey, SVMInstruction<'a>)>,
) -> PrecompileSignatureDetails {
    let mut filter = SignatureDetailsFilter::new();

    // Wrapping arithmetic is safe below because the maximum number of signatures
    // per instruction is 255, and the maximum number of instructions per transaction
    // is low enough that the sum of all signatures will not overflow a u64.
    let mut num_secp256k1_instruction_signatures: u64 = 0;
    let mut num_ed25519_instruction_signatures: u64 = 0;
    for (program_id, instruction) in instructions {
        let program_id_index = instruction.program_id_index;
        match filter.is_signature(program_id_index, program_id) {
            ProgramIdStatus::NotSignature => {}
            ProgramIdStatus::Secp256k1 => {
                num_secp256k1_instruction_signatures = num_secp256k1_instruction_signatures
                    .wrapping_add(get_num_signatures_in_instruction(&instruction));
            }
            ProgramIdStatus::Ed25519 => {
                num_ed25519_instruction_signatures = num_ed25519_instruction_signatures
                    .wrapping_add(get_num_signatures_in_instruction(&instruction));
            }
        }
    }

    PrecompileSignatureDetails {
        num_secp256k1_instruction_signatures,
        num_ed25519_instruction_signatures,
    }
}

#[inline]
fn get_num_signatures_in_instruction(instruction: &SVMInstruction) -> u64 {
    u64::from(instruction.data.first().copied().unwrap_or(0))
}

#[derive(Copy, Clone)]
enum ProgramIdStatus {
    NotSignature,
    Secp256k1,
    Ed25519,
}

struct SignatureDetailsFilter {
    // array of slots for all possible static and sanitized program_id_index,
    // each slot indicates if a program_id_index has not been checked, or is
    // already checked with result that can be reused.
    flags: [Option<ProgramIdStatus>; FILTER_SIZE as usize],
}

impl SignatureDetailsFilter {
    #[inline]
    fn new() -> Self {
        Self {
            flags: [None; FILTER_SIZE as usize],
        }
    }

    #[inline]
    fn is_signature(&mut self, index: u8, program_id: &Pubkey) -> ProgramIdStatus {
        let flag = &mut self.flags[usize::from(index)];
        match flag {
            Some(status) => *status,
            None => {
                *flag = Some(Self::check_program_id(program_id));
                *flag.as_ref().unwrap()
            }
        }
    }

    #[inline]
    fn check_program_id(program_id: &Pubkey) -> ProgramIdStatus {
        if program_id == &solana_sdk::secp256k1_program::ID {
            ProgramIdStatus::Secp256k1
        } else if program_id == &solana_sdk::ed25519_program::ID {
            ProgramIdStatus::Ed25519
        } else {
            ProgramIdStatus::NotSignature
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // simple convenience function so avoid having inconsistent program_id and program_id_index
    fn make_instruction<'a>(
        program_ids: &'a [Pubkey],
        program_id_index: u8,
        data: &'a [u8],
    ) -> (&'a Pubkey, SVMInstruction<'a>) {
        (
            &program_ids[program_id_index as usize],
            SVMInstruction {
                program_id_index,
                accounts: &[],
                data,
            },
        )
    }

    #[test]
    fn test_get_signature_details_no_instructions() {
        let instructions = std::iter::empty();
        let signature_details = get_precompile_signature_details(instructions);

        assert_eq!(signature_details.num_secp256k1_instruction_signatures, 0);
        assert_eq!(signature_details.num_ed25519_instruction_signatures, 0);
    }

    #[test]
    fn test_get_signature_details_no_sigs_unique() {
        let program_ids = [Pubkey::new_unique(), Pubkey::new_unique()];
        let instructions = [
            make_instruction(&program_ids, 0, &[]),
            make_instruction(&program_ids, 1, &[]),
        ];

        let signature_details = get_precompile_signature_details(instructions.into_iter());
        assert_eq!(signature_details.num_secp256k1_instruction_signatures, 0);
        assert_eq!(signature_details.num_ed25519_instruction_signatures, 0);
    }

    #[test]
    fn test_get_signature_details_signatures_mixed() {
        let program_ids = [
            Pubkey::new_unique(),
            solana_sdk::secp256k1_program::ID,
            solana_sdk::ed25519_program::ID,
        ];
        let instructions = [
            make_instruction(&program_ids, 1, &[5]),
            make_instruction(&program_ids, 2, &[3]),
            make_instruction(&program_ids, 0, &[]),
            make_instruction(&program_ids, 2, &[2]),
            make_instruction(&program_ids, 1, &[1]),
            make_instruction(&program_ids, 0, &[]),
        ];

        let signature_details = get_precompile_signature_details(instructions.into_iter());
        assert_eq!(signature_details.num_secp256k1_instruction_signatures, 6);
        assert_eq!(signature_details.num_ed25519_instruction_signatures, 5);
    }

    #[test]
    fn test_get_signature_details_missing_num_signatures() {
        let program_ids = [
            solana_sdk::secp256k1_program::ID,
            solana_sdk::ed25519_program::ID,
        ];
        let instructions = [
            make_instruction(&program_ids, 0, &[]),
            make_instruction(&program_ids, 1, &[]),
        ];

        let signature_details = get_precompile_signature_details(instructions.into_iter());
        assert_eq!(signature_details.num_secp256k1_instruction_signatures, 0);
        assert_eq!(signature_details.num_ed25519_instruction_signatures, 0);
    }
}
