use {
    crate::utils,
    solana_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        instruction::{AccountMeta, Instruction},
        message::Message,
        signature::Signer,
        signer::keypair::{read_keypair_file, Keypair},
        transaction::Transaction,
    },
};

/// Establishes a RPC connection with the Simulation server.
/// Information about the server is gleened from the config file `config.yml`.
pub fn establish_connection(url: &Option<&str>, config: &Option<&str>) -> utils::Result<RpcClient> {
    let rpc_url = match url {
        Some(x) => {
            if *x == "localhost" {
                "http://localhost:8899".to_string()
            } else {
                String::from(*x)
            }
        }
        None => utils::get_rpc_url(config)?,
    };
    Ok(RpcClient::new_with_commitment(
        rpc_url,
        CommitmentConfig::confirmed(),
    ))
}

/// Loads keypair information from the file located at KEYPAIR_PATH
/// and then verifies that the loaded keypair information corresponds
/// to an executable account via CONNECTION. Failure to read the
/// keypair or the loaded keypair corresponding to an executable
/// account will result in an error being returned.
pub fn get_program(keypair_path: &str, connection: &RpcClient) -> utils::Result<Keypair> {
    let program_keypair = read_keypair_file(keypair_path).map_err(|e| {
        utils::Error::InvalidConfig(format!(
            "failed to read program keypair file ({}): ({})",
            keypair_path, e
        ))
    })?;

    let program_info = connection.get_account(&program_keypair.pubkey())?;
    if !program_info.executable {
        return Err(utils::Error::InvalidConfig(format!(
            "program with keypair ({}) is not executable",
            keypair_path
        )));
    }

    Ok(program_keypair)
}

pub fn say_hello(player: &Keypair, program: &Keypair, connection: &RpcClient) -> utils::Result<()> {
    let greeting_pubkey = utils::get_greeting_public_key(&player.pubkey(), &program.pubkey())?;
    println!("greeting pubkey {greeting_pubkey:?}");

    // Submit an instruction to the chain which tells the program to
    // run. We pass the account that we want the results to be stored
    // in as one of the account arguments which the program will
    // handle.

    let data = [1u8];
    let instruction = Instruction::new_with_bytes(
        program.pubkey(),
        &data,
        vec![AccountMeta::new(greeting_pubkey, false)],
    );
    let message = Message::new(&[instruction], Some(&player.pubkey()));
    let transaction = Transaction::new(&[player], message, connection.get_latest_blockhash()?);

    let response = connection.simulate_transaction(&transaction)?;
    println!("{:?}", response);

    Ok(())
}
