//! Bitcoin SPV proof verifier program
//! Receive merkle proofs and block headers, validate transaction
use crate::spv_instruction::*;
use crate::spv_state::*;
#[allow(unused_imports)]
use crate::utils::*;
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
        let testheaderbytes = decode_hex(&testheader)?;
        let testhashbytes = decode_hex(&testhash)?;

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
        let testdata = "010000008a730974ac39042e95f82d719550e224c1a680a8dc9e8df9d007000000000000f50b20e8720a552dd36eb2ebdb7dceec9569e0395c990c1eb8a4292eeda05a931e1fce4e9a110e1a7a58aeb01601000000010000000000000000000000000000000000000000000000000000000000000000ffffffff5370736a049a110e1a04b099a417522cfabe6d6d4e6988c831bb48c551eea50f87b3c6461ade476fe15c98bed7c6a574aca4ff3501000000000000004d696e656420627920425443204775696c6420ac1eeeed88ffffffff0140ff082a010000001976a914ca975b00a8c203b8692f5a18d92dc5c2d2ebc57b88ac000000000100000001d81a8ff9114a09536c43b16003011a91bce5a2941d117e050c01ba1920181c72010000008c493046022100c7784a417a5780b922dd6385bd1cc07b530794c63d6b0584378fc6dfb79d35d50221008812fd23e25549fca2ea6273e0f609f8aa93e2e822ec9a4537d57cd4fb664165014104cfe9363f2c7213bde611e57a4e16b2fb90cf3db160276e5f9f12081c718ebb3821a485866585b3fe416d1b28d4fd993db339dd38bef48da7ec4db5b618a1ce09ffffffff02404b4c00000000001976a91499e8943edcd38645f4a99a7173c9c42ab3e2160c88ac440730ec010000001976a9142e61d7641959b2afcaae323186c239d65df1e58488ac00000000010000000171b250f62a6a5a17ab0a0d60592c39871ba2141a166a852c94cf4f00f0fcb88d010000008b48304502202e1f5fc8065c38a5d7471aa7d3f495048ea773049e6f30d939173dd553fa114f0221008fad13d287ead5c297589211ec0b687fe129f7ff61a344825b295a5c0eda06450141047ef7874043d8355b39ca58c4894ff937c9bce598ac01b325407694d8ddd21323e4e032ea316727aa8e85b242ad1c0d545067dcece333506b2d8953ae1bb353a2ffffffff0258062dc4010000001976a914ccf183c4a7a42a6c867679a83d51b772d235804c88ac404b4c00000000001976a9145503819d5e9e2e895d55324e628d3b1af815466d88ac0000000001000000019e159267ed2a94b05a2490e529aa05f81f161746a8d7538d8d083404aed25b18010000008a4730440220120cc710fd8159ac3fe91f38da71fd02cc2059fc01492ecc7cc585ffb7ad87640220210663325d88bbfa8970e39278c6a2ea7e9fb847e015aa7eb9524c6e823c1dce0141040fa1e62b7dafc01c1a1955dcb64771fa52a542ae1ffd4e27a0c1a17506bc9f7c284cdac5f143533da5b16fd5c3ed46db685f02779114ae04e93d77543e41d6e6ffffffff02a67782bd010000001976a9149f5ce0036d4789e31aafaf6f2fdd868dfa5b1ae088ac003fab01000000001976a914cc40482478b48bd4d9b58811158578f4781afac788ac000000000100000001f5b1b9f9c42877b87a26cceced1e99586634559d039d80e74a99151dbc00d717000000008a47304402207a60b11e229acf40e6220795db7333ccce51f80ea8d6647f660802877bf2720302205da11ec43cdf2c5691dc6a06bed69ff66358180a34fbe4dda35ea89ba2b0dcf801410406cf22457b397612b2a17d784eafb7e9791fdd280bf7611ca208a1d3cba93c0000ae288e479a65b78c9cbb1085cdc8527ce32b2140141047e49344093109d61dffffffff0269b3efe6010000001976a9145589689a604344109a68d69df6381164b461ceb088ac404b4c00000000001976a9145a142400d119bc83324be75e4bdaa10bc185f34588ac00000000010000000149f4fa9c09f0c84f752a692b5e4e07d12008d0b94929f2fa034de55e147a7dd0000000008a47304402206cf4fe9ed2a001f97ef78e1e6eef9064e7231519b78e5dca0d0d11ccd83f8c9b02206d4a92ed2329cc4cfdbedb24e92011bf421e8e4d063d2e9e97c823eea9fab9dc014104733cbdcc4e2ae95cfe41649484e673c717ec345c6a826c7c7456890cd1b5e7cfcf060067e7e2aa7c3cb3642179461d0a2f15f2fb42e68e83ee49d23e64433192ffffffff0100e40b54020000001976a91454182395494dede8d425bcc2e6fbf7546b786bd288ac000000000100000001c2c792496e56e2031dc7d5c06523891f54d2302fb029a6843487f5855f396aa5000000008b48304502210094f4b7b9390850068f10b3ac8b1cf75af254d8bcd25ec4d12a5b520c86f67b5a022027cbd4721fb6266ff8d3672223f102ac873c4cf2eb2d9296a33fed3664ebf68a014104aff25466888145adefd4a1c112e5177c9b0af932285986523f929cd11e976d11dec9bbac7e391b31b7e9fa07a24aa7ae100f7ffbb1ef864929973f411086bcb2ffffffff0200ac274f000000001976a9141627a6be6590113c2732336397684479d565a7f388ac007f3e36020000001976a914a3d0bcdd84e99472951cc444358ccebac61b486688ac0000000001000000011d8de29c9296e611320ca394f163641a8778d0e9868315c342b4085ec8d59ab0010000008b483045022100a818131085d4370d021a3ae74fd4f8a18cf1c360b15745ab741d8ae0fa77355d02207c460fba7989c293006546326b7b1aeeb2c1e561c5951fb73fdee580d9988a6b0141043872ee2f87817b3dd607998d9476b86726a82f4c1489c94ee2dd6b7a8473c301547d0f8cae1980ef96b995d7db0eb69e9005dc03ab0f156ee7a5e4d36ff62a15ffffffff0200bfb6b3030000001976a9143379b7e05199f19ac136e77298ebd06ee5b1e5c888ac00222048020000001976a914b57e8eea7de80cf79f947bd63a6faeeb59f1f3cd88ac000000000100000001c783c90af206590a0012b4ce6d249083c485bf96b78c83a62dbfcf541c9c84ba010000008a473044022001123857ff1e3a668fadd9c3acac426e111e8ab30b5c2f12d822d364dbedb06c022001dfc1adb694973795b36227b82f82b93d2c39ebf5f1d5e3d729d632c93451740141044a1cd8f265e0a566dc79cf7ea6daf0c29fc2e6281d97a6c495c86a6a4408f6b09ebdae41c8586e70ebfb01acfccee0b5c2d5e5ed5c39cd3644f9bba306f2cc61ffffffff02404b4c00000000001976a9140c6c6c888d64b60fa7d5707f6eb7359048dba59588acb0c40f3f000000001976a914771ba703ec68903093a4fbe3ec812cda8d23ea0088ac0000000001000000021f51f82586c92c57f1839c6b15367600f4555ac38f16de1f2fdf67c61f636fad010000008b4830450221008981eee0c8a82e58335f0c6a0a0d06bae1b8084ebe3f2451f14c271380c5641902206c26b4c5d4cc86c0fe5d38de661324c92b64fdc77873aeddc798332da4655134014104591f9853eb931b5d084801190d282ab7f5c8abbb299f5749d6b105715866e1767c081e8454e9ea3bf63a19c09b39dc1ff71d93270afa94b062a4984af9ad2dbcffffffffa55e28ebbcf94453ff11acb57b4b427c8e429f3a2e5441d63db2afdcb350fbc8010000008a473044022036431306f32b48e3b892d3dd9aa4deec0f5ea1dbabfc106cbe7b44cdbad83ad3022026a19c9093f946a28daa57df2ac9583dc13519d3c6b852cc3bcd2c479565e0b601410467c6736641546582785ca6f0d4e56c08fe67ee882c7907bf16471c85daf96ef48ce804a10a142175ce7d3be7d3bdcf77c884795441aaad7554a92a1ab47f000affffffff0280969800000000001976a91459d3fad18e55f2ffccb6829e67e77cb0e5b8e4d388ac00943577000000001976a9142cb4491ea630d4d5938f340baad7e31e4942626688ac0000000001000000069d74a193c94fa2d9aa9ed6a5d534defa99fe009cd2785d72c358b17bd36763c0010000008b483045022100d9912fadefc20eeb2c59fd10848367542b4065f9da044841361d6c28280fcfd702205078310c595e9efb65670f82b8b1fc07d498df1d56ba74c2186ea0ee8fbc8c11014104f1a2ed99e816a18a92beb211e312b7d07d0a64b8c55739f6b308240b8eae077d9ce5c616c6c32a50d26c04746a67cc6bce7f3466caf153146e883baeb403bf3affffffffb8cba6398365c2667249c5714cfaf0bf71c62b17874db28c582be377e5a408ed010000008b483045022100b386d0a462c3d84dde82ece6e011917370add71d3ac9bbf1a7f6800803d27f7b022060ce9deeb23b08f1658df36fa2333403e83116cb0803919cfd0776e2641af1f5014104f1a2ed99e816a18a92beb211e312b7d07d0a64b8c55739f6b308240b8eae077d9ce5c616c6c32a50d26c04746a67cc6bce";
        //sorry
        let testhash = "5b09bbb8d3cb2f8d4edbcf30664419fb7c9deaeeb1f62cb432e7741c80dbe5ba";

        let testdatabytes = decode_hex(&testdata).unwrap();
        let header = &testdatabytes[0..80];
        let mut txdata = &testdatabytes[80..];

        let vilen = measure_variable_int(&txdata[0..9]).unwrap();
        let txnum = decode_variable_int(&txdata[0..9]).unwrap();

        txdata = &txdata[vilen..];
        let tx = Transaction::new(txdata.to_vec());

        assert_eq!(tx.inputs.len(), 1);

        assert_eq!(tx.outputs.len(), 1);

        assert_eq!(tx.version, 1);
    }
}
