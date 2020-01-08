use clap;
use clap::{App, Arg};
use hex;
use reqwest;
use std::fs::File;
use std::io::prelude::*;

fn get_block_raw(hash: &str) -> String {
    let qs = format!("https://blockchain.info/block/{}?format=hex", hash);
    let body = reqwest::blocking::get(&qs);
    match body {
        Err(e) => panic!("rest request failed {}", e),
        Ok(n) => {
            if n.status().is_success() {
                n.text().unwrap()
            } else {
                panic!("request failed");
            }
        }
    }
}

fn write_file(fname: String, bytes: &[u8]) -> std::io::Result<()> {
    let mut buffer = File::create(fname)?;
    buffer.write_all(bytes)?;
    Ok(())
}

fn main() {
    let matches = App::new("header fetch util")
        .arg(Arg::with_name("blockhash"))
        .arg(Arg::with_name("output"))
        .help("block hash to get header from")
        .get_matches();

    let default_output = "file";
    let output = matches.value_of("output").unwrap_or(default_output);

    let testhash = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
    let blockhash = matches.value_of("blockhash").unwrap_or(testhash);
    let blockraw = get_block_raw(&blockhash);

    if default_output == output {
        let fname = format!("block-{}.in", blockhash);
        let outf = hex::decode(&blockraw).unwrap();
        let arr = &outf[0..];
        write_file(fname, arr).unwrap();
    } else {
        println!("{}", blockraw);
    }
}
