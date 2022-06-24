#![allow(clippy::integer_arithmetic)]

use solana_runtime::genesis_utils::ValidatorVoteKeypairs;

#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    std::sync::Arc,
    solana_sdk::{
        signature::{Signer, Keypair},
        native_token::LAMPORTS_PER_SOL,
    },
};

pub fn main() {
    solana_logger::setup_with_default("info");
    info!("suhhh");

    let priv_node_key: &[u8] = &[232, 216, 7, 12, 130, 87, 248, 13, 41, 47, 182, 219, 230, 218, 35, 24, 
                                65, 87, 42, 211, 220, 253, 31, 144, 91, 236, 82, 86, 147, 124, 207, 190, 
                                166, 88, 56, 42, 28, 22, 140, 124, 46, 146, 5, 112, 155, 180, 33, 96, 
                                43, 38, 97, 68, 150, 25, 54, 135, 146, 41, 243, 84, 91, 156, 188, 133];

    let priv_vote_key: &[u8] = &[202, 35, 250, 0, 21, 185, 178, 195, 95, 109, 163, 124, 249, 63, 82, 196, 
                                154, 15, 247, 26, 127, 15, 41, 162, 97, 252, 18, 33, 166, 14, 88, 18, 37, 
                                147, 252, 70, 228, 141, 254, 161, 251, 179, 207, 232, 54, 226, 190, 223, 
                                77, 17, 229, 79, 193, 210, 131, 85, 231, 169, 0, 162, 22, 93, 90, 24];

    let priv_stake_key: &[u8] = &[121, 154, 185, 120, 121, 119, 208, 158, 154, 183, 96, 104, 87, 79, 191, 
                                165, 63, 20, 14, 148, 221, 196, 216, 66, 107, 110, 214, 125, 102, 38, 5, 
                                92, 114, 72, 21, 228, 214, 107, 206, 150, 75, 47, 110, 206, 203, 71, 220, 
                                164, 238, 82, 204, 75, 100, 86, 106, 26, 78, 2, 46, 90, 204, 224, 21, 230];

    let stake: u64 = 10 * LAMPORTS_PER_SOL; 

    let node_keypair = Keypair::from_bytes(&priv_node_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();
    let vote_keypair = Keypair::from_bytes(&priv_vote_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();
    let stake_keypair = Keypair::from_bytes(&priv_stake_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();


    let mut node_keys: Vec<Keypair> = Vec::new();
    let mut vote_keys: Vec<Keypair> = Vec::new();
    let mut stake_keys: Vec<Keypair> = Vec::new();
    let mut stakes: Vec<u64> = Vec::new();

    node_keys.push(node_keypair);
    vote_keys.push(vote_keypair);
    stake_keys.push(stake_keypair);
    stakes.push(stake);

    let(keys_in_genesis, stakes_in_genesis): (Vec<ValidatorVoteKeypairs>, Vec<u64>) = 
        node_keys
            .iter()
            .zip(&vote_keys)
            .zip(&stake_keys)
            .filter_map


    let key = Arc::new(Keypair::new());
    info!("key: {}", key.pubkey());
    info!("secret: {:?}", key.secret());
    info!("key bytes: {:?}", key.to_bytes());


}
