//! Defines account apis for use in Solana programs.

use crate::account::KeyedAccount;
use crate::credit_only_account::KeyedCreditOnlyAccount;
use crate::instruction::InstructionError;
use crate::pubkey::Pubkey;
use std::fmt::Debug;
use std::sync::atomic::Ordering;

pub trait AccountApi: Debug {
    fn signer_key(&self) -> Option<&Pubkey>;
    fn unsigned_key(&self) -> &Pubkey;
    fn lamports(&self) -> u64;
    fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError>;
    fn credit(&mut self, lamports: u64) -> Result<(), InstructionError>;
    fn debit(&mut self, lamports: u64) -> Result<(), InstructionError>;
    fn get_data(&self) -> &[u8];
    fn account_writer(&mut self) -> Result<&mut [u8], InstructionError>;
    fn initialize_data(&mut self, space: u64) -> Result<(), InstructionError>;
    fn owner(&self) -> &Pubkey;
    fn set_owner(&mut self, owner: &Pubkey) -> Result<(), InstructionError>;
    fn is_executable(&self) -> bool;
    fn set_executable(&mut self, value: bool) -> Result<(), InstructionError>;
}

impl<'a> AccountApi for KeyedAccount<'a> {
    fn signer_key(&self) -> Option<&Pubkey> {
        self.signer_key()
    }
    fn unsigned_key(&self) -> &Pubkey {
        self.unsigned_key()
    }
    fn lamports(&self) -> u64 {
        self.account.lamports
    }
    fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.account.lamports = lamports;
        Ok(())
    }
    fn credit(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.account.lamports += lamports;
        Ok(())
    }
    fn debit(&mut self, lamports: u64) -> Result<(), InstructionError> {
        if self.account.lamports < lamports {
            Err(InstructionError::new_result_with_negative_lamports())
        } else {
            self.account.lamports -= lamports;
            Ok(())
        }
    }
    fn get_data(&self) -> &[u8] {
        &self.account.data
    }
    fn account_writer(&mut self) -> Result<&mut [u8], InstructionError> {
        Ok(&mut self.account.data)
    }
    fn initialize_data(&mut self, space: u64) -> Result<(), InstructionError> {
        self.account.data = vec![0; space as usize];
        Ok(())
    }
    fn owner(&self) -> &Pubkey {
        &self.account.owner
    }
    fn set_owner(&mut self, owner: &Pubkey) -> Result<(), InstructionError> {
        self.account.owner = *owner;
        Ok(())
    }
    fn is_executable(&self) -> bool {
        self.account.executable
    }
    fn set_executable(&mut self, value: bool) -> Result<(), InstructionError> {
        self.account.executable = value;
        Ok(())
    }
}

impl<'a> AccountApi for KeyedCreditOnlyAccount<'a> {
    fn signer_key(&self) -> Option<&Pubkey> {
        self.signer_key()
    }
    fn unsigned_key(&self) -> &Pubkey {
        self.unsigned_key()
    }
    fn lamports(&self) -> u64 {
        self.account.lamports.load(Ordering::Relaxed) + self.credits
    }
    fn set_lamports(&mut self, lamports: u64) -> Result<(), InstructionError> {
        let current_lamports = self.account.lamports.load(Ordering::Relaxed);
        if lamports >= current_lamports {
            let credit = lamports - current_lamports;
            self.credits += credit;
            Ok(())
        } else {
            Err(InstructionError::CreditOnlyDebit)
        }
    }
    fn credit(&mut self, lamports: u64) -> Result<(), InstructionError> {
        self.credits += lamports;
        Ok(())
    }
    fn debit(&mut self, _lamports: u64) -> Result<(), InstructionError> {
        Err(InstructionError::CreditOnlyDebit)
    }
    fn get_data(&self) -> &[u8] {
        &self.account.data
    }
    fn account_writer(&mut self) -> Result<&mut [u8], InstructionError> {
        Err(InstructionError::CreditOnlyDataModification)
    }
    fn initialize_data(&mut self, _space: u64) -> Result<(), InstructionError> {
        Err(InstructionError::CreditOnlyDataModification)
    }
    fn owner(&self) -> &Pubkey {
        &self.account.owner
    }
    fn set_owner(&mut self, _owner: &Pubkey) -> Result<(), InstructionError> {
        Err(InstructionError::CreditOnlyOwnerModification)
    }
    fn is_executable(&self) -> bool {
        self.account.executable
    }
    fn set_executable(&mut self, _value: bool) -> Result<(), InstructionError> {
        Err(InstructionError::CreditOnlyDataModification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Account;
    use crate::credit_only_account::CreditOnlyAccount;
    use bincode::serialize_into;
    use std::sync::Arc;

    #[test]
    fn test_account_api() {
        let pubkey0 = Pubkey::new_rand();
        let mut account0 = Account::new(1, 0, &Pubkey::default());
        let mut keyed_account = KeyedAccount::new(&pubkey0, false, &mut account0);

        let pubkey1 = Pubkey::new_rand();
        let account1 = Account::new(2, std::mem::size_of::<u32>(), &Pubkey::default());
        let credit_account = Arc::new(CreditOnlyAccount::from(account1));
        let mut keyed_credit_account = KeyedCreditOnlyAccount::new(&pubkey1, true, &credit_account);

        let mut collection: Vec<&mut AccountApi> =
            vec![&mut keyed_account, &mut keyed_credit_account];

        assert_eq!(collection[0].signer_key(), None);
        assert_eq!(collection[1].signer_key(), Some(&pubkey1));

        assert_eq!(collection[0].unsigned_key(), &pubkey0);
        assert_eq!(collection[1].unsigned_key(), &pubkey1);

        assert_eq!(collection[0].lamports(), 1);
        assert_eq!(collection[1].lamports(), 2);

        assert!(collection[0].set_lamports(10).is_ok());
        assert_eq!(collection[0].lamports(), 10);
        assert!(collection[1].set_lamports(11).is_ok());
        assert_eq!(collection[1].lamports(), 11);
        assert!(collection[1].set_lamports(0).is_err());
        assert_eq!(collection[1].lamports(), 11);

        assert!(collection[0].credit(10).is_ok());
        assert_eq!(collection[0].lamports(), 20);
        assert!(collection[1].credit(10).is_ok());
        assert_eq!(collection[1].lamports(), 21);

        assert!(collection[0].debit(10).is_ok());
        assert_eq!(collection[0].lamports(), 10);
        assert!(collection[1].debit(10).is_err());
        assert_eq!(collection[1].lamports(), 21);

        assert!(collection[0].initialize_data(4).is_ok());
        assert_eq!(collection[0].get_data(), &[0; 4]);
        assert!(collection[1].initialize_data(8).is_err());
        assert_eq!(collection[1].get_data(), &[0; 4]);

        assert!(serialize_into(collection[0].account_writer().unwrap(), &42u32).is_ok());
        assert_eq!(collection[0].get_data(), &[42, 0, 0, 0]);
        assert!(collection[1].account_writer().is_err());

        let new_pubkey = Pubkey::new_rand();
        assert!(collection[0].set_owner(&new_pubkey).is_ok());
        assert_eq!(collection[0].owner(), &new_pubkey);
        assert!(collection[1].set_owner(&new_pubkey).is_err());
        assert_eq!(collection[1].owner(), &Pubkey::default());

        assert!(collection[0].set_executable(true).is_ok());
        assert_eq!(collection[0].is_executable(), true);
        assert!(collection[1].set_executable(true).is_err());
        assert_eq!(collection[1].is_executable(), false);

        assert_eq!(
            account0,
            Account {
                lamports: 10,
                data: vec![42, 0, 0, 0],
                owner: new_pubkey,
                executable: true,
            }
        );
    }
}
