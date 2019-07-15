//! Bitcoin SPV proof verifier program
//! Receive merkle proofs and block headers, validate transaction

use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::InstructionError;


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
            let header    = *headers[bh]
            let pheader   = *headers[bh-1];
            // check that the headerchain is in order and contiguous
            if header.parent != pheader.digest {
                error!(format!("Invalid Header Chain hash sequence for header index {}", bh));
                Err(InstructionError::InvalidArgument)?
            }
            //check that block difficulty is above the given threshold
            if header.difficulty < diff {
                error!(format!("Invalid block difficulty for header index {}", bh));
                Err(InstructionError::InvalidArgument)?
            }
        }
        //not done yet, needs difficulty average/variance checking still
        Ok(())
    }



}
pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let command = bincode::deserialize::<SpvInstruction>(data).map_err(|err| {
        info!("invalid transaction data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    trace!("{:?}", command);

    match command{
        SpvInstruction::VerificationRequest() => {
            SpvProcessor::do_verification_request()
        }
    }


}
