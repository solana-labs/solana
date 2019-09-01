//! Bitcoin SPV proof verifier program
//! Receive merkle proofs and block headers, validate transaction
#[allow(unused_imports)]
use crate::spv_instruction::*;
use crate::spv_state::*;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
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
        keyed_accounts: &mut [KeyedAccount],
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

    pub fn do_cancel_request(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        if keyed_accounts.len() != 2 {
            error!("Client Request invalid accounts argument length (should be 2)")
        }
        const OWNER_INDEX: usize = 0;
        const CANCEL_INDEX: usize = 1;
        Ok(()) //placeholder
    }

    pub fn do_submit_proof(
        keyed_accounts: &mut [KeyedAccount],
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
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    // solana_logger::setup();

    let command = bincode::deserialize::<SpvInstruction>(data).map_err(|err| {
        info!("invalid instruction data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

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
        let testhashbytes = decode_hex(&testhash)?;
        let mut thb: [u8; 32] = Default::default();
        thb.copy_from_slice(&testhashbytes[..32]);

        let bh = BlockHeader::hexnew(&testheader, &testhash)?;
        // let mut bbh: [u8;32] = Default::default();
        // bbh.copy_from_slice(bh[.. bh.len()]);

        assert_eq!(bh.blockhash, thb);
        Ok(())
    }
}
