use std::error::Error;
use solana_sdk::{
    signature::{
        Keypair,
        Signature,
    },
    signer::Signer,
    transaction::Transaction,
    system_instruction,
    system_transaction, commitment_config::{CommitmentConfig, CommitmentLevel},
};
use solana_client::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_remote_keypair::openpgp_card::{OpenpgpCardKeypair, OpenpgpCardInfo};

fn _test_sig() {
    println!("Hello from Solana sdk!");

    let sk = "5mU6wMD64nQ8AwjFrkL2qQnoyvQcJNEE8K5GUQ1GcyCysZWYWxqSxPsLdi16nCT7hyLvHrhWMjeTfp9A9AJWUi7a";
    let kp = Keypair::from_base58_string(&sk);
    let sig = kp.sign_message(b"ABC def 123.");
    print!("Signature: {:?}", sig);
    //for byte in sig {
    //    print!("{:02X?}", byte);
    //}
    println!();
}

fn request_airdrop(rpc: &RpcClient, pk: &Pubkey, lamports: u64) -> Result<Signature, Box<dyn Error>> {
    let sig = rpc.request_airdrop(&pk, lamports)?;
    rpc.confirm_transaction_with_spinner(
        &sig,
        &rpc.get_latest_blockhash()?,
        CommitmentConfig {
            commitment: CommitmentLevel::Finalized,
        }
    )?;
    Ok(sig)
}

fn _true_transact() -> Result<(), Box<dyn Error>> {
    println!("Hello from Solana SDK!");

    let rpc_client = RpcClient::new("https://api.devnet.solana.com");

    let sk = "5mU6wMD64nQ8AwjFrkL2qQnoyvQcJNEE8K5GUQ1GcyCysZWYWxqSxPsLdi16nCT7hyLvHrhWMjeTfp9A9AJWUi7a";
    let payer = Keypair::from_base58_string(&sk);
    let new_account = Keypair::new();

    println!("Requesting airdrop...");
    let _ = request_airdrop(&rpc_client, &payer.pubkey(), 1_000_000_000);
    println!("Payer: {}", payer.pubkey());
    println!("Receiver: {}", new_account.pubkey());

    let txid = rpc_client.send_and_confirm_transaction(
        &system_transaction::transfer(
            &payer,
            &new_account.pubkey(),
            100_000_000,
            rpc_client.get_latest_blockhash()?
        )
    )?;
    println!("Transaction: {}", txid);

    Ok(())
}

fn yubi_transact() -> Result<(), Box<dyn Error>> {
    println!("================================================================================");
    println!("I LOVE YUBIKEY!!!!!!!!!!!!!!!!!!");

    let rpc_client = RpcClient::new("https://api.devnet.solana.com");

    //let sk = "5mU6wMD64nQ8AwjFrkL2qQnoyvQcJNEE8K5GUQ1GcyCysZWYWxqSxPsLdi16nCT7hyLvHrhWMjeTfp9A9AJWUi7a";
    let payer = OpenpgpCardKeypair::new_from_identifier(None)?;
    let new_account = Keypair::new();

    println!("--------------------------------------------------------------------------------");
    println!("Requesting airdrop...");
    request_airdrop(&rpc_client, &payer.pubkey(), 1_000_000_000)?;
    println!("Received 1 SOL");

    println!("--------------------------------------------------------------------------------");
    println!("Payer pubkey: {} (YUBIKEY)", payer.pubkey());
    println!("Receiver pubkey: {}", new_account.pubkey());
    println!("--------------------------------------------------------------------------------");

    // use std::{thread, time};
    // let ten_millis = time::Duration::from_secs(10);
    // let now = time::Instant::now();
    // thread::sleep(ten_millis);

    let txid = rpc_client.send_and_confirm_transaction_with_spinner(
        &Transaction::new_signed_with_payer(
            &[
                system_instruction::transfer(
                    &payer.pubkey(),
                    &new_account.pubkey(),
                    100_000_000
                )
            ],
            Some(&payer.pubkey()),
            &[&payer],
            rpc_client.get_latest_blockhash()?
        ),
    )?;

    println!("--------------------------------------------------------------------------------");
    println!("Transaction: {}", txid);

    Ok(())
}

fn _list_cards() -> Result<(), Box<dyn Error>> {
    let card_infos = OpenpgpCardInfo::find_cards()?;
    for card_info in card_infos {
        println!("================================================================================");
        println!("{:#?}", card_info);
    }
    Ok(())
}

fn main() {
    yubi_transact().unwrap();
}
