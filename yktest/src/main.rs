use std::error::Error;
use std::io::{stdin, stdout, Read, Write};
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
use solana_remote_keypair::openpgp_card::{OpenpgpCardKeypair, OpenpgpCardInfo, Locator};

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

fn _yubi_transact() -> Result<(), Box<dyn Error>> {
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

fn pause() {
    let mut stdout = stdout();
    stdout.write(b"Press Enter to continue...").unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}

fn yubi_exchange() -> Result<(), Box<dyn Error>> {
    println!("================================================================================");
    println!("I LOVE YUBIKEY!!!!!!!!!!!!!!!!!!");

    let rpc_client = RpcClient::new("https://api.devnet.solana.com");

    //let sk = "5mU6wMD64nQ8AwjFrkL2qQnoyvQcJNEE8K5GUQ1GcyCysZWYWxqSxPsLdi16nCT7hyLvHrhWMjeTfp9A9AJWUi7a";
    let big_yubi_uri = uriparse::URIReference::try_from("pgpcard://D2760001240103040006205304730000")?;
    let big_yubi = OpenpgpCardKeypair::new_from_locator(
        Locator::new_from_uri(&big_yubi_uri)?
    )?;
    let small_yubi_uri = uriparse::URIReference::try_from("pgpcard://D2760001240100000006223637020000")?;
    let small_yubi = OpenpgpCardKeypair::new_from_locator(
        Locator::new_from_uri(&small_yubi_uri)?
    )?;

    println!("--------------------------------------------------------------------------------");
    println!("Requesting airdrop (1 SOL) into big Yubikey...");
    request_airdrop(&rpc_client, &big_yubi.pubkey(), 1_000_000_000)?;
    println!("Received 1 SOL");

    println!("--------------------------------------------------------------------------------");
    println!("Big Yubikey balance: {}", rpc_client.get_balance(&big_yubi.pubkey())? as f64 / 1_000_000_000.0);
    println!("Small Yubikey balance: {}", rpc_client.get_balance(&small_yubi.pubkey())? as f64 / 1_000_000_000.0);

    println!("--------------------------------------------------------------------------------");
    println!("Transfer 0.5 SOL from big Yubikey --> small Yubikey");
    println!("Payer pubkey: {}", big_yubi.pubkey());
    println!("Receiver pubkey: {}", small_yubi.pubkey());
    println!("--------------------------------------------------------------------------------");
    pause();

    let txid = rpc_client.send_and_confirm_transaction_with_spinner(
        &Transaction::new_signed_with_payer(
            &[
                system_instruction::transfer(
                    &big_yubi.pubkey(),
                    &small_yubi.pubkey(),
                    500_000_000
                )
            ],
            Some(&big_yubi.pubkey()),
            &[&big_yubi],
            rpc_client.get_latest_blockhash()?
        ),
    )?;
    println!("Transaction: {}", txid);
    pause();

    ///////////////////////////////////////////////////////////////////////////////////////////////

    println!("--------------------------------------------------------------------------------");
    println!("Transfer 0.1 SOL from small Yubikey --> big Yubikey");
    println!("Payer pubkey: {}", small_yubi.pubkey());
    println!("Receiver pubkey: {}", big_yubi.pubkey());
    println!("--------------------------------------------------------------------------------");
    pause();

    let txid = rpc_client.send_and_confirm_transaction_with_spinner(
        &Transaction::new_signed_with_payer(
            &[
                system_instruction::transfer(
                    &small_yubi.pubkey(),
                    &big_yubi.pubkey(),
                    100_000_000
                )
            ],
            Some(&small_yubi.pubkey()),
            &[&small_yubi],
            rpc_client.get_latest_blockhash()?
        ),
    )?;
    println!("Transaction: {}", txid);

    Ok(())
}

fn main() {
    yubi_exchange().unwrap();
}
