//! Bitcoin SPV proof verifier program
//! Receive merkle proofs and block headers, validate transaction
use crate::spv_state::*;
use crate::spv_instruction::*;
use crate::utils::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::InstructionError;
use solana_sdk::account::KeyedAccount;
use log::*;


pub struct SpvProcessor {}

impl SpvProcessor {

    pub fn validateHeaderChain(headers: &HeaderChain, proof_req: &ProofRequest) -> Result<(), InstructionError> {
        let ln       = *headers.len();
        let confirms = *proof_req.confirmations;
        let diff     = *proof_req.difficulty;
        // check that the headerchain is the right length
        if ln != (2 + confirms) {
            error!("Invalid length for Header Chain");
            Err(InstructionError::InvalidArgument)? //is this ? necessary?
        }

        for bh in 1..ln {
            let header    = *headers[bh];
            let pheader   = *headers[bh-1];
            // check that the headerchain is in order and contiguous
            if header.parent != pheader.digest {
                error!("Invalid Header Chain hash sequence for header index {}", bh);
                Err(InstructionError::InvalidArgument)?
            }
            //check that block difficulty is above the given threshold
            if header.difficulty < diff {
                error!("Invalid block difficulty for header index {}", bh);
                Err(InstructionError::InvalidArgument)?
            }
        }
        //not done yet, needs difficulty average/variance checking still
        Ok(())
    }

    fn deserialize_proof(data: &[u8]) -> Result<ProofInfo, InstructionError> {
        let proof_state : AccountState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg);
        if let AccountState::Verification(Proof) = proof_state {
            Ok(proof_state)
        } else {
            error!("Not a valid proof");
            Err(InstructionError::InvalidAccountData)?
        }
    }

    fn deserialize_request(data: &[u8]) -> Result<ClientRequestInfo, InstructionError> {
        let req_state : AccountState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg);
        if let AccountState::Request(info) = req_state {
            Ok(req_state)
        } else {
            error!("Not a valid proof request");
            Err(InstructionError::InvalidAccountData)?;
        }
    }

    pub fn check_account_unallocated(data: &[u8]) -> Result<(), InstructionError> {
        let acct_state : AccountState = bincode::deserialize(data).map_err(Self::map_to_invalid_arg);
        if let AccountState::Unallocated = acct_state {
            Ok(())
        } else {
            error!("Provided account is already occupied");
            Err(InstructionError::InvalidAccountData)?;
        }
    }


    pub fn do_client_request(
        keyed_accounts: &mut [KeyedAccount],
        request_info  : &ClientRequestInfo,
    ) -> Result<(), InstructionError> {
        if *keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const OWNER_INDEX   : usize = 0;
        const REQUEST_INDEX : usize = 1;

        Self.check_account_unallocated(&keyed_accounts[REQUEST_INDEX].account.data)?;
        Ok(()) //placeholder
    }

    pub fn do_cancel_request(
        keyed_accounts: &mut[KeyedAccount],
        txHash        : BitcoinTxHash,
    ) -> Result<(), InstructionError> {
        if *keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const OWNER_INDEX: usize = 0;
        const CANCEL_INDEX: usize = 1;
        Ok(()) //placeholder
    }


    pub fn do_submit_proof(
        keyed_accounts: &mut[KeyedAccount],
        proof_info    : &ProofInfo,
    ) -> Result<(), InstructionError> {
        if *keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const SUBMITTER_INDEX: usize = 0;
        const PROOF_REQUEST_INDEX: usize = 1;
        Ok(()) //placeholder
    }

}
pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    // solana_logger::setup();

    let command = bincode::deserialize::<SpvInstruction>(data).map_err(|err| {
        info!("invalid instruction data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    trace!("{:?}", command);

    match command{
        SpvInstruction::ClientRequest(client_request_info) => {
            SpvProcessor::client_request(keyed_accounts, &client_request_info)
        }
        SpvInstruction::CancelRequest => {
            SpvProcessor::cancel_request(keyed_accounts)
        }
        SpvInstruction::SubmitProof(proof_info) => {
            SpvProcessor::submit_request(keyed_accounts, &proof_info)
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
        let testhash   = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
        let testhashbytes = decode_hex(&testhash)?;

        let bh = BlockHeader::hexnew(&testheader, &testhash)?;
        assert_eq!(bh.blockhash, testhashbytes);
        Ok(())
    }
}
