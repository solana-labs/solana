use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::invoke,
    pubkey::Pubkey,
};

const ARGUMENT_INDEX: usize = 0;

const INSTRUCTION_MODIFY: u8 = 0;
const INSTRUCTION_INVOKE_MODIFY: u8 = 1;
const INSTRUCTION_MODIFY_INVOKE: u8 = 2;
const INSTRUCTION_VERIFY_MODIFIED: u8 = 3;

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    assert!(!accounts[ARGUMENT_INDEX].is_writable);

    match instruction_data[0] {
        INSTRUCTION_MODIFY => {
            msg!("modify ro account");
            assert_eq!(0, accounts[ARGUMENT_INDEX].try_borrow_data()?[0]);
            accounts[ARGUMENT_INDEX].try_borrow_mut_data()?[0] = 1;
        }
        INSTRUCTION_INVOKE_MODIFY => {
            msg!("invoke and modify ro account");

            assert_eq!(0, accounts[ARGUMENT_INDEX].try_borrow_data()?[0]);

            let instruction = Instruction {
                program_id: *program_id,
                accounts: vec![AccountMeta::new_readonly(
                    *accounts[ARGUMENT_INDEX].key,
                    false,
                )],
                data: vec![INSTRUCTION_MODIFY],
            };
            invoke(&instruction, accounts)?;
        }
        INSTRUCTION_MODIFY_INVOKE => {
            msg!("modify and invoke ro account");

            assert_eq!(0, accounts[ARGUMENT_INDEX].try_borrow_data()?[0]);
            accounts[ARGUMENT_INDEX].try_borrow_mut_data()?[0] = 1;

            let instruction = Instruction {
                program_id: *program_id,
                accounts: vec![AccountMeta::new_readonly(
                    *accounts[ARGUMENT_INDEX].key,
                    false,
                )],
                data: vec![INSTRUCTION_VERIFY_MODIFIED],
            };
            invoke(&instruction, accounts)?;
        }
        INSTRUCTION_VERIFY_MODIFIED => {
            msg!("verify modified");
            assert_eq!(1, accounts[ARGUMENT_INDEX].try_borrow_data()?[0])
        }
        _ => panic!("Unknown instruction"),
    }
    Ok(())
}
