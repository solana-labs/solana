use log::*;
use solana_sdk::{
    account::KeyedAccount,
    burn_instruction::BurnInstruction,
    instruction::InstructionError,
    program_utils::{limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
};

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError> {
    let instruction = limited_deserialize(instruction_data)?;

    trace!("process_instruction: {:?}", instruction);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    match instruction {
        BurnInstruction::Burn => {
            let keyed_accounts_iter = &mut keyed_accounts.iter();
            let account = next_keyed_account(keyed_accounts_iter)?;
            account
                .signer_key()
                .ok_or(InstructionError::MissingRequiredSignature)?;
            account.try_account_ref_mut()?.lamports = 0;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    #[test]
    fn test_burn_ok() {
        let from_account = Account::new_ref(100, 0, &Pubkey::default());

        assert_eq!(from_account.borrow().lamports, 100);
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(&Pubkey::new_rand(), true, &from_account),],
                &bincode::serialize(&BurnInstruction::Burn).unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.borrow().lamports, 0);
    }

    #[test]
    fn test_burn_no_signer() {
        let from_account = Account::new_ref(100, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(&Pubkey::new_rand(), false, &from_account),],
                &bincode::serialize(&BurnInstruction::Burn).unwrap()
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_burn_no_account() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &[],
                &bincode::serialize(&BurnInstruction::Burn).unwrap()
            ),
            Err(InstructionError::NotEnoughAccountKeys)
        );
    }

    #[test]
    fn test_burn_bad_instruction() {
        assert_eq!(
            process_instruction(&Pubkey::default(), &[], &[1, 2, 3],),
            Err(InstructionError::InvalidInstructionData)
        );
    }
}
