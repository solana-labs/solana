use crate::{
    account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction, pubkey::Pubkey,
};

/// Invoke a cross-program instruction
///
/// Note that the program id of the instruction being issued must also be included in
/// `account_infos`.
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
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
        extern "C" {
            fn sol_invoke_signed_rust(
                instruction_addr: *const u8,
                account_infos_addr: *const u8,
                account_infos_len: u64,
                signers_seeds_addr: *const u8,
                signers_seeds_len: u64,
            ) -> u64;
        }

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

/// Maximum size that can be set using sol_set_return_data()
pub const MAX_RETURN_DATA: usize = 1024;

/// Set a program's return data
pub fn set_return_data(data: &[u8]) {
    #[cfg(target_arch = "bpf")]
    {
        extern "C" {
            fn sol_set_return_data(data: *const u8, length: u64);
        }

        unsafe { sol_set_return_data(data.as_ptr(), data.len() as u64) };
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_set_return_data(data)
}

/// Get the return data from invoked program
pub fn get_return_data() -> Option<(Pubkey, Vec<u8>)> {
    #[cfg(target_arch = "bpf")]
    {
        use std::cmp::min;

        extern "C" {
            fn sol_get_return_data(data: *mut u8, length: u64, program_id: *mut Pubkey) -> u64;
        }

        let mut buf = [0u8; MAX_RETURN_DATA];
        let mut program_id = Pubkey::default();

        let size =
            unsafe { sol_get_return_data(buf.as_mut_ptr(), buf.len() as u64, &mut program_id) };

        if size == 0 {
            None
        } else {
            let size = min(size as usize, MAX_RETURN_DATA);
            Some((program_id, buf[..size as usize].to_vec()))
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_get_return_data()
}
