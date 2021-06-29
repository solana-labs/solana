use crate::{account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction};

/// BPF syscall error when invoke_signed_with_budget fails due to
/// CPI exceeding caller-specified compute units
pub const BPF_STATUS_CODE_1__CPI_COMPUTE_BUDGET_EXCEEDED: u32 = 0x0b9f_05c1;

/// Invoke a cross-program instruction
///
/// Note that the program id of the instruction being issued must also be included in
/// `account_infos`.
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
}

/// Invoke a cross-program instruction with a caller-specified compute budget
///
/// Note that the program id of the instruction being issued must also be included in
/// `account_infos`.
pub fn invoke_with_budget(
    instruction: &Instruction,
    budget: u64,
    account_infos: &[AccountInfo],
) -> ProgramResult {
    invoke_signed_with_budget(instruction, budget, account_infos, &[])
}

/// Invoke a cross-program instruction with program signatures
///
/// Note that the program id of the instruction being issued must also be included in
/// `account_infos`.
pub fn invoke_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    // Check that the account RefCells are consistent with the request
    for account_meta in instruction.accounts.iter() {
        for account_info in account_infos.iter() {
            if account_meta.pubkey == *account_info.key {
                if account_meta.is_writable {
                    let _ = account_info.try_borrow_mut_lamports()?;
                    let _ = account_info.try_borrow_mut_data()?;
                } else {
                    let _ = account_info.try_borrow_lamports()?;
                    let _ = account_info.try_borrow_data()?;
                }
                break;
            }
        }
    }

    #[cfg(target_arch = "bpf")]
    {
        let result = unsafe {
            sol_invoke_signed_rust(
                instruction as *const _ as *const u8,
                account_infos as *const _ as *const u8,
                account_infos.len() as u64,
                signers_seeds as *const _ as *const u8,
                signers_seeds.len() as u64,
            )
        };
        match result {
            crate::entrypoint::SUCCESS => Ok(()),
            _ => Err(result.into()),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_invoke_signed(instruction, account_infos, signers_seeds)
}

/// Invoke a cross-program instruction with program signatures and
/// with a caller-specified compute budget
///
/// Note that the program id of the instruction being issued must also be included in
/// `account_infos`.
///

pub fn invoke_signed_with_budget(
    instruction: &Instruction,
    _budget: u64,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    // Check that the account RefCells are consistent with the request
    for account_meta in instruction.accounts.iter() {
        for account_info in account_infos.iter() {
            if account_meta.pubkey == *account_info.key {
                if account_meta.is_writable {
                    let _ = account_info.try_borrow_mut_lamports()?;
                    let _ = account_info.try_borrow_mut_data()?;
                } else {
                    let _ = account_info.try_borrow_lamports()?;
                    let _ = account_info.try_borrow_data()?;
                }
                break;
            }
        }
    }

    #[cfg(target_arch = "bpf")]
    {
        let budgeted_instruction = crate::instruction::BudgetedInstruction {
            budget: _budget,
            instruction_addr: instruction as *const _ as u64,
        };
        let result = unsafe {
            sol_invoke_signed_with_budget_rust(
                &budgeted_instruction as *const _ as *const u8,
                account_infos as *const _ as *const u8,
                account_infos.len() as u64,
                signers_seeds as *const _ as *const u8,
                signers_seeds.len() as u64,
            )
        };
        match result {
            crate::entrypoint::SUCCESS => Ok(()),
            _ => Err(result.into()),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_invoke_signed(instruction, account_infos, signers_seeds)
}

#[cfg(target_arch = "bpf")]
extern "C" {
    fn sol_invoke_signed_rust(
        instruction_addr: *const u8,
        account_infos_addr: *const u8,
        account_infos_len: u64,
        signers_seeds_addr: *const u8,
        signers_seeds_len: u64,
    ) -> u64;
}

#[cfg(target_arch = "bpf")]
extern "C" {
    fn sol_invoke_signed_with_budget_rust(
        instruction_addr: *const u8,
        account_infos_addr: *const u8,
        account_infos_len: u64,
        signers_seeds_addr: *const u8,
        signers_seeds_len: u64,
    ) -> u64;
}
