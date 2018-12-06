//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

extern crate bincode;
extern crate env_logger;
#[macro_use]
extern crate log;
#[macro_use]
extern crate solana_sdk;

use bincode::deserialize;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::storage_program::*;
use std::sync::{Once, ONCE_INIT};

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    argdata: &[u8],
    _input: &[u8],
    _tick_height: u64,
) -> Result<Vec<u8>, ProgramError> {
    static INIT: Once = ONCE_INIT;
    INIT.call_once(|| {
        // env_logger can only be initialized once
        env_logger::init();
    });

    // accounts_keys[0] must be signed
    if keyed_accounts[0].signer_key().is_none() {
        info!("account[0] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    if let Ok(syscall) = deserialize(argdata) {
        match syscall {
            StorageProgram::SubmitMiningProof { sha_state } => {
                info!("Mining proof submitted with state {:?}", sha_state);
            }
        }
        Ok(vec![])
    } else {
        info!("Invalid instruction argdata: {:?}", argdata);
        Err(ProgramError::InvalidArgumentsData)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::account::create_keyed_accounts;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_storage_tx() {
        let keypair = Keypair::new();
        let mut accounts = [(keypair.pubkey(), Default::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(entrypoint(&id(), &mut keyed_accounts, &[], 42).is_err());
    }
}
