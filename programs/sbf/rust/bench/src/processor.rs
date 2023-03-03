//#![cfg(feature = "program")]

use {
    crate::instructions::*,
    solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        program::invoke,
        pubkey::Pubkey,
    },
};

solana_program::entrypoint!(process_instruction);

fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Bench processor");

    match BenchInstruction::try_from(instruction_data).unwrap() {
        BenchInstruction::Nop => msg!("nop"),
        BenchInstruction::ReadAccounts(r) => read_accounts(accounts, r),
        BenchInstruction::WriteAccounts(w) => write_accounts(accounts, w),
        BenchInstruction::Recurse(r) => recurse(program_id, accounts, r.n),
    };

    Ok(())
}

fn read_accounts(accounts: &[AccountInfo], r: ReadAccounts) {
    msg!("read_accounts");

    let mut read = 0u64;
    for account in accounts.iter().take(r.num_accounts) {
        let data = account.data.borrow();
        msg!("account len {}", data.len());
        for x in 0..r.size {
            read = read.saturating_add(data[x] as u64);
        }
    }

    // this is so the whole thing doesn't get optimized away
    msg!("read {}", read);
}

fn write_accounts(accounts: &[AccountInfo], w: WriteAccounts) {
    msg!("write_accounts");

    for account in accounts.iter().take(w.num_accounts) {
        let mut data = account.data.borrow_mut();
        for x in 0..w.size {
            data[x] = x as u8;
        }
    }
}

fn recurse(program_id: &Pubkey, accounts: &[AccountInfo], n: usize) {
    msg!("recurse {}", n);

    if n > 0 {
        let instruction = Instruction {
            program_id: *program_id,
            accounts: accounts
                .iter()
                .map(|a| AccountMeta {
                    pubkey: *a.key,
                    is_writable: a.is_writable,
                    is_signer: a.is_signer,
                })
                .collect(),
            data: BenchInstruction::Recurse(Recurse {
                n: n.saturating_sub(1),
            })
            .to_bytes(),
        };
        invoke(&instruction, accounts).unwrap();
    }
}
