//! Example Rust-based SBF realloc test program

#![cfg(feature = "program")]

extern crate solana_program;
use {
    crate::instructions::*,
    solana_program::{
        account_info::AccountInfo,
        entrypoint::{ProgramResult, MAX_PERMITTED_DATA_INCREASE},
        instruction::{AccountMeta, Instruction},
        msg,
        program::invoke,
        pubkey::Pubkey,
        system_instruction, system_program,
    },
    solana_sbf_rust_realloc::instructions::*,
    std::convert::TryInto,
};

solana_program::entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let account = &accounts[0];
    let invoke_program_id = accounts[1].key;
    let pre_len = account.data_len();
    let mut bump = 0;

    match instruction_data[0] {
        INVOKE_REALLOC_ZERO_RO => {
            msg!("invoke realloc to zero of ro account");
            // Realloc RO account
            let mut instruction = realloc(invoke_program_id, account.key, 0, &mut bump);
            instruction.accounts[0].is_writable = false;
            invoke(&instruction, accounts)?;
        }
        INVOKE_REALLOC_ZERO => {
            msg!("invoke realloc to zero");
            invoke(
                &realloc(invoke_program_id, account.key, 0, &mut bump),
                accounts,
            )?;
            assert_eq!(0, account.data_len());
        }
        INVOKE_REALLOC_MAX_PLUS_ONE => {
            msg!("invoke realloc max + 1");
            invoke(
                &realloc(
                    invoke_program_id,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE.saturating_add(1),
                    &mut bump,
                ),
                accounts,
            )?;
        }
        INVOKE_REALLOC_EXTEND_MAX => {
            msg!("invoke realloc max");
            invoke(
                &realloc_extend(
                    invoke_program_id,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE,
                    &mut bump,
                ),
                accounts,
            )?;
            assert_eq!(
                pre_len.saturating_add(MAX_PERMITTED_DATA_INCREASE),
                account.data_len()
            );
        }
        INVOKE_REALLOC_TO_THEN_LOCAL_REALLOC_EXTEND => {
            let (bytes, remaining_data) =
                instruction_data[2..].split_at(std::mem::size_of::<usize>());
            let new_len = usize::from_le_bytes(bytes.try_into().unwrap());
            msg!("invoke realloc to {} byte(s)", new_len);
            let realloc_to_ix = {
                let mut instruction_data = vec![INVOKE_REALLOC_TO, 1];
                instruction_data.extend_from_slice(&new_len.to_le_bytes());

                Instruction::new_with_bytes(
                    *invoke_program_id,
                    &instruction_data,
                    vec![
                        AccountMeta::new(*account.key, false),
                        AccountMeta::new_readonly(*invoke_program_id, false),
                    ],
                )
            };
            invoke(&realloc_to_ix, accounts)?;
            assert_eq!(new_len, account.data_len());
            let (bytes, _) = remaining_data.split_at(std::mem::size_of::<usize>());
            let extend_len = usize::from_le_bytes(bytes.try_into().unwrap());
            msg!("realloc extend {} byte(s)", extend_len);
            account.realloc(new_len.saturating_add(extend_len), false)?;
            assert_eq!(new_len.saturating_add(extend_len), account.data_len());
        }
        INVOKE_REALLOC_MAX_TWICE => {
            msg!("invoke realloc max twice");
            invoke(
                &realloc(
                    invoke_program_id,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE,
                    &mut bump,
                ),
                accounts,
            )?;
            let new_len = pre_len.saturating_add(MAX_PERMITTED_DATA_INCREASE);
            assert_eq!(new_len, account.data_len());
            account.realloc(new_len.saturating_add(MAX_PERMITTED_DATA_INCREASE), false)?;
            assert_eq!(
                new_len.saturating_add(MAX_PERMITTED_DATA_INCREASE),
                account.data_len()
            );
        }
        INVOKE_REALLOC_AND_ASSIGN => {
            msg!("invoke realloc and assign");
            invoke(
                &Instruction::new_with_bytes(
                    *invoke_program_id,
                    &[REALLOC_AND_ASSIGN],
                    vec![AccountMeta::new(*account.key, false)],
                ),
                accounts,
            )?;
            assert_eq!(
                pre_len.saturating_add(MAX_PERMITTED_DATA_INCREASE),
                account.data_len()
            );
            assert_eq!(*account.owner, system_program::id());
        }
        INVOKE_REALLOC_AND_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM => {
            msg!("invoke realloc and assign to self via system program");
            invoke(
                &Instruction::new_with_bytes(
                    *accounts[1].key,
                    &[REALLOC_AND_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM],
                    vec![
                        AccountMeta::new(*account.key, true),
                        AccountMeta::new_readonly(*accounts[2].key, false),
                    ],
                ),
                accounts,
            )?;
        }
        INVOKE_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM_AND_REALLOC => {
            msg!("invoke assign to self and realloc via system program");
            invoke(
                &Instruction::new_with_bytes(
                    *accounts[1].key,
                    &[ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM_AND_REALLOC],
                    vec![
                        AccountMeta::new(*account.key, true),
                        AccountMeta::new_readonly(*accounts[2].key, false),
                    ],
                ),
                accounts,
            )?;
        }
        INVOKE_REALLOC_INVOKE_CHECK => {
            msg!("realloc invoke check size");
            account.realloc(100, false)?;
            assert_eq!(100, account.data_len());
            account.try_borrow_mut_data()?[pre_len..].fill(2);
            invoke(
                &Instruction::new_with_bytes(
                    *accounts[1].key,
                    &[CHECK],
                    vec![AccountMeta::new(*account.key, false)],
                ),
                accounts,
            )?;
        }
        INVOKE_REALLOC_TO => {
            let (bytes, _) = instruction_data[2..].split_at(std::mem::size_of::<usize>());
            let new_len = usize::from_le_bytes(bytes.try_into().unwrap());
            msg!("realloc to {}", new_len);
            account.realloc(new_len, false)?;
            assert_eq!(new_len, account.data_len());
            if pre_len < new_len {
                account.try_borrow_mut_data()?[pre_len..].fill(instruction_data[1]);
            }
        }
        INVOKE_REALLOC_RECURSIVE => {
            msg!("realloc invoke recursive");
            let (bytes, _) = instruction_data[2..].split_at(std::mem::size_of::<usize>());
            let new_len = usize::from_le_bytes(bytes.try_into().unwrap());
            account.realloc(new_len, false)?;
            assert_eq!(new_len, account.data_len());
            account.try_borrow_mut_data()?[pre_len..].fill(instruction_data[1]);
            let final_len: usize = 200;
            let mut new_instruction_data = vec![];
            new_instruction_data.extend_from_slice(&[INVOKE_REALLOC_TO, 2]);
            new_instruction_data.extend_from_slice(&final_len.to_le_bytes());
            invoke(
                &Instruction::new_with_bytes(
                    *program_id,
                    &new_instruction_data,
                    vec![
                        AccountMeta::new(*account.key, false),
                        AccountMeta::new_readonly(*accounts[1].key, false),
                    ],
                ),
                accounts,
            )?;
            assert_eq!(final_len, account.data_len());
            let data = account.try_borrow_mut_data()?;
            for i in 0..new_len {
                assert_eq!(data[i], instruction_data[1]);
            }
            for i in new_len..final_len {
                assert_eq!(data[i], new_instruction_data[1]);
            }
        }
        INVOKE_CREATE_ACCOUNT_REALLOC_CHECK => {
            msg!("Create new account, realloc, and check");
            let pre_len: usize = 100;
            invoke(
                &system_instruction::create_account(
                    accounts[0].key,
                    accounts[1].key,
                    3000000, // large enough for rent exemption
                    pre_len as u64,
                    program_id,
                ),
                accounts,
            )?;
            assert_eq!(pre_len, accounts[1].data_len());
            accounts[1].realloc(pre_len.saturating_add(1), false)?;
            assert_eq!(pre_len.saturating_add(1), accounts[1].data_len());
            assert_eq!(accounts[1].owner, program_id);
            let final_len: usize = 200;
            let mut new_instruction_data = vec![];
            new_instruction_data.extend_from_slice(&[INVOKE_REALLOC_TO, 2]);
            new_instruction_data.extend_from_slice(&final_len.to_le_bytes());
            invoke(
                &Instruction::new_with_bytes(
                    *program_id,
                    &new_instruction_data,
                    vec![
                        AccountMeta::new(*accounts[1].key, false),
                        AccountMeta::new_readonly(*accounts[3].key, false),
                    ],
                ),
                accounts,
            )?;
            assert_eq!(final_len, accounts[1].data_len());
        }
        INVOKE_DEALLOC_AND_ASSIGN => {
            msg!("realloc zerod");
            let (bytes, _) = instruction_data[2..].split_at(std::mem::size_of::<usize>());
            let pre_len = usize::from_le_bytes(bytes.try_into().unwrap());
            let new_len = pre_len.saturating_mul(2);
            assert_eq!(pre_len, 100);
            {
                let data = account.try_borrow_mut_data()?;
                for i in 0..pre_len {
                    assert_eq!(data[i], instruction_data[1]);
                }
            }

            invoke(
                &Instruction::new_with_bytes(
                    *accounts[2].key,
                    &[DEALLOC_AND_ASSIGN_TO_CALLER],
                    vec![
                        AccountMeta::new(*account.key, false),
                        AccountMeta::new_readonly(*accounts[1].key, false),
                    ],
                ),
                accounts,
            )?;
            assert_eq!(account.owner, program_id);
            assert_eq!(account.data_len(), 0);
            account.realloc(new_len, false)?;
            assert_eq!(account.data_len(), new_len);
            {
                let data = account.try_borrow_mut_data()?;
                for i in 0..new_len {
                    assert_eq!(data[i], 0);
                }
            }
        }
        INVOKE_REALLOC_MAX_INVOKE_MAX => {
            msg!("invoke realloc max invoke max");
            assert_eq!(0, account.data_len());
            account.realloc(MAX_PERMITTED_DATA_INCREASE, false)?;
            assert_eq!(MAX_PERMITTED_DATA_INCREASE, account.data_len());
            account.assign(invoke_program_id);
            assert_eq!(account.owner, invoke_program_id);
            invoke(
                &realloc_extend(
                    invoke_program_id,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE,
                    &mut bump,
                ),
                accounts,
            )?;
        }
        INVOKE_INVOKE_MAX_TWICE => {
            msg!("invoke invoke max twice");
            assert_eq!(0, account.data_len());
            account.assign(accounts[2].key);
            assert_eq!(account.owner, accounts[2].key);
            invoke(
                &realloc(
                    accounts[2].key,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE,
                    &mut bump,
                ),
                accounts,
            )?;
            invoke(
                &realloc_extend(
                    accounts[2].key,
                    account.key,
                    MAX_PERMITTED_DATA_INCREASE,
                    &mut bump,
                ),
                accounts,
            )?;
            panic!("last invoke should fail");
        }
        _ => panic!(),
    }

    Ok(())
}
