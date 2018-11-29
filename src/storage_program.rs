//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

use bincode::deserialize;
use program::ProgramError;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StorageProgram {
    SubmitMiningProof { sha_state: Hash },
}

pub enum StorageError {
    InvalidArgument,
    InvalidUserData,
}

const STORAGE_PROGRAM_ID: [u8; 32] = [
    130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == STORAGE_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&STORAGE_PROGRAM_ID)
}

pub fn get_balance(account: &Account) -> u64 {
    account.tokens
}

pub fn process_instruction(
    tx: &Transaction,
    pix: usize,
    _accounts: &mut [&mut Account],
) -> Result<(), StorageError> {
    // accounts_keys[0] must be signed
    if tx.signer_key(pix, 0).is_none() {
        Err(StorageError::InvalidArgument)?;
    }

    if let Ok(syscall) = deserialize(tx.userdata(pix)) {
        match syscall {
            StorageProgram::SubmitMiningProof { sha_state } => {
                info!("Mining proof submitted with state {:?}", sha_state);
                return Ok(());
            }
        }
    } else {
        return Err(StorageError::InvalidUserData);
    }
}

pub fn process(
    tx: &Transaction,
    instruction_index: usize,
    accounts: &mut [&mut Account],
) -> std::result::Result<(), ProgramError> {
    process_instruction(&tx, instruction_index, accounts).map_err(|_| ProgramError::GenericError)
}

#[cfg(test)]
mod test {
    use super::*;
    use signature::Keypair;
    use signature::KeypairUtil;

    #[test]
    fn test_storage_tx() {
        let keypair = Keypair::new();
        let tx = Transaction::new(&keypair, &[], id(), &(), Default::default(), 0);
        assert!(process_instruction(&tx, 0, &mut []).is_err());
    }
}
