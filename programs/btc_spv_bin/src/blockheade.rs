use clap;
use clap::{App, Arg};
use reqwest;
use serde_derive::Deserialize;

// pub type blockHash = [u8; 32];
pub type BlockHeader = [u8; 80];

#[allow(dead_code)]
#[derive(Deserialize)]
struct JsonBH {
    hash: String,
    ver: u16,
    prev_block: String,
    mrkl_root: String,
    time: u64,
    bits: u64,
    nonce: u64,
    n_tx: u64,
    size: u64,
    block_index: u64,
    main_chain: bool,
    height: u64,
    received_time: u64,
    relayed_by: String,
}

#[allow(dead_code)]
fn get_header_json(hash: &str) -> JsonBH {
    let qs = format!("https://www.blockchain.info/rawblock/{}", hash);
    let body = reqwest::get(&qs);
    match body {
        Err(e) => panic!("rest request failed {}", e),
        Ok(mut n) => {
            if n.status().is_success() {
                let jsonbh: JsonBH = n.json().unwrap();
                jsonbh
            } else {
                panic!("request failed");
            }
        }
    }
}

fn get_header_raw(hash: &str) -> String {
    let qs = format!("https://blockchain.info/block/{}?format=hex", hash);
    let body = reqwest::get(&qs);
    match body {
        Err(e) => panic!("rest request failed {}", e),
        Ok(mut n) => {
            if n.status().is_success() {
                let textbh: String = n.text().unwrap();
                let hs = &textbh[0..160]; // 160 characters since it's in hex format
                let header: String = hs.to_string();
                header
            } else {
                panic!("request failed");
            }
        }
    }
}

fn main() {
    let matches = App::new("header fetch util")
        .arg(Arg::with_name("blockhash"))
        .help("block hash to get header from")
        .get_matches();

    let testhash = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
    let blockhash = matches.value_of("blockhash").unwrap_or(testhash);
    let headerraw = get_header_raw(&blockhash);
    println!("header - {}", headerraw);
    println!("hash   - {}", blockhash);
    println!("length - {}", headerraw.len());
    // println!("{}", std::str::from_utf8(&header));
}
