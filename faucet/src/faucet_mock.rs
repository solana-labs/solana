use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_transaction,
    transaction::Transaction,
};
use std::{
    io::{Error, ErrorKind},
    net::SocketAddr,
};

pub fn request_airdrop_transaction(
    _faucet_addr: &SocketAddr,
    _id: &Pubkey,
    lamports: u64,
    _blockhash: Hash,
) -> Result<Transaction, Error> {
    if lamports == 0 {
        Err(Error::new(ErrorKind::Other, "Airdrop failed"))
    } else {
        let key = Keypair::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, lamports, blockhash);
        Ok(tx)
    }
}
