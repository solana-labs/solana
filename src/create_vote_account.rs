use bank::Bank;
use result::Result;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_transaction::*;

pub fn create_vote_account(
    node_keypair: &Keypair,
    bank: &Bank,
    num_tokens: u64,
    last_id: Hash,
) -> Result<Keypair> {
    let new_vote_account = Keypair::new();

    // Create and register the new vote account
    let tx = Transaction::vote_account_new(
        node_keypair,
        new_vote_account.pubkey(),
        last_id,
        num_tokens,
        0,
    );
    bank.process_transaction(&tx)?;

    Ok(new_vote_account)
}
