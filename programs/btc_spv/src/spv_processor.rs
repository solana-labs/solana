//! Bitcoin SPV proof verifier program
//! Receive merkle proofs and block headers, validate transaction
use crate::spv_instruction::*;
use crate::spv_state::*;
#[allow(unused_imports)]
use crate::utils::*;
use hex;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::limited_deserialize;
use solana_sdk::pubkey::Pubkey;

pub struct SpvProcessor {}

impl SpvProcessor {
    pub fn validate_header_chain(
        headers: HeaderChain,
        proof_req: &ProofRequest,
    ) -> Result<(), InstructionError> {
        // disabled for time being
        //not done yet, needs difficulty average/variance checking still
        Ok(())
    }

    #[allow(clippy::needless_pass_by_value)]
    fn map_to_invalid_arg(err: std::boxed::Box<bincode::ErrorKind>) -> InstructionError {
        warn!("Deserialize failed, not a valid state: {:?}", err);
        InstructionError::InvalidArgument
    }

    fn deserialize_proof(data: &[u8]) -> Result<Proof, InstructionError> {
        let proof_state: AccountState =
            bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let AccountState::Verification(proof) = proof_state {
            Ok(proof)
        } else {
            error!("Not a valid proof");
            Err(InstructionError::InvalidAccountData)
        }
    }

    fn deserialize_request(data: &[u8]) -> Result<ClientRequestInfo, InstructionError> {
        let req_state: AccountState =
            bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let AccountState::Request(info) = req_state {
            Ok(info)
        } else {
            error!("Not a valid proof request");
            Err(InstructionError::InvalidAccountData)
        }
    }

    pub fn check_account_unallocated(data: &[u8]) -> Result<(), InstructionError> {
        let acct_state: AccountState =
            bincode::deserialize(data).map_err(Self::map_to_invalid_arg)?;
        if let AccountState::Unallocated = acct_state {
            Ok(())
        } else {
            error!("Provided account is already occupied");
            Err(InstructionError::InvalidAccountData)
        }
    }

    pub fn do_client_request(
        keyed_accounts: &[KeyedAccount],
        request_info: &ClientRequestInfo,
    ) -> Result<(), InstructionError> {
        if keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const OWNER_INDEX: usize = 0;
        const REQUEST_INDEX: usize = 1;

        // check_account_unallocated(&keyed_accounts[REQUEST_INDEX].account.data)?;
        Ok(()) //placeholder
    }

    pub fn do_cancel_request(keyed_accounts: &[KeyedAccount]) -> Result<(), InstructionError> {
        if keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const OWNER_INDEX: usize = 0;
        const CANCEL_INDEX: usize = 1;
        Ok(()) //placeholder
    }

    pub fn do_submit_proof(
        keyed_accounts: &[KeyedAccount],
        proof_info: &Proof,
    ) -> Result<(), InstructionError> {
        if keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const SUBMITTER_INDEX: usize = 0;
        const PROOF_REQUEST_INDEX: usize = 1;
        Ok(()) //placeholder
    }
}
pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    // solana_logger::setup();

    let command = limited_deserialize::<SpvInstruction>(data)?;

    trace!("{:?}", command);

    match command {
        SpvInstruction::ClientRequest(client_request_info) => {
            SpvProcessor::do_client_request(keyed_accounts, &client_request_info)
        }
        SpvInstruction::CancelRequest => SpvProcessor::do_cancel_request(keyed_accounts),
        SpvInstruction::SubmitProof(proof_info) => {
            SpvProcessor::do_submit_proof(keyed_accounts, &proof_info)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{spv_instruction, spv_state, utils};

    #[test]
    fn test_parse_header_hex() -> Result<(), SpvError> {
        let testheader = "010000008a730974ac39042e95f82d719550e224c1a680a8dc9e8df9d007000000000000f50b20e8720a552dd36eb2ebdb7dceec9569e0395c990c1eb8a4292eeda05a931e1fce4e9a110e1a7a58aeb0";
        let testhash = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
        let testheaderbytes = hex::decode(&testheader)?;
        let testhashbytes = hex::decode(&testhash)?;

        let mut blockhash: [u8; 32] = [0; 32];
        blockhash.copy_from_slice(&testhashbytes[..32]);

        let mut version: [u8; 4] = [0; 4];
        version.copy_from_slice(&testheaderbytes[..4]);
        let test_version = u32::from_le_bytes(version);

        let mut test_parent: [u8; 32] = [0; 32];
        test_parent.copy_from_slice(&testheaderbytes[4..36]);

        let mut merkleroot: [u8; 32] = [0; 32];
        merkleroot.copy_from_slice(&testheaderbytes[36..68]);

        let mut time: [u8; 4] = [0; 4];
        time.copy_from_slice(&testheaderbytes[68..72]);
        let test_time = u32::from_le_bytes(time);

        let mut test_nonce: [u8; 4] = [0; 4];
        test_nonce.copy_from_slice(&testheaderbytes[76..80]);

        let bh = BlockHeader::hexnew(&testheader, &testhash)?;

        assert_eq!(bh.blockhash, blockhash);
        assert_eq!(bh.merkle_root.hash, merkleroot);
        assert_eq!(bh.version, test_version);
        assert_eq!(bh.time, test_time);
        assert_eq!(bh.parent, test_parent);
        assert_eq!(bh.nonce, test_nonce);

        Ok(())
    }

    #[test]
    fn test_parse_transaction_hex() {
        let testblockhash = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
        let testtxhash = "5b09bbb8d3cb2f8d4edbcf30664419fb7c9deaeeb1f62cb432e7741c80dbe5ba";

        let mut testdatabytes = include_bytes!("testblock.in");
        let mut headerbytes = hex::encode(&testdatabytes[0..]);
        let hbc = &headerbytes[0..80];

        let mut txdata = &testdatabytes[80..];

        let vilen = measure_variable_int(&txdata[0..9]).unwrap();
        let txnum = decode_variable_int(&txdata[0..9]).unwrap();

        txdata = &txdata[vilen..];
        let tx = BitcoinTransaction::new(txdata.to_vec());

        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(txnum, 22);
        assert_eq!(tx.outputs.len(), 1);
        assert_eq!(tx.version, 1);
    }
}
