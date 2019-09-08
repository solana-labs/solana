use clap;
use clap::{App, Arg};


fn get_block_raw(hash: &str) -> String {
    let qs = format!("https://blockchain.info/block/{}?format=hex", hash);
    let body = ureq::get(&qs).call();
    if body.error() {
        panic!("request failed");
    } else {
        let textbh: String = body.into_string().unwrap();
        textbh
    }
}

fn main() {
    let matches = App::new("header fetch util")
        .arg(Arg::with_name("blockhash"))
        .help("block hash to get header from")
        .get_matches();

    let testhash = "0000000000000bae09a7a393a8acded75aa67e46cb81f7acaa5ad94f9eacd103";
    let blockhash = matches.value_of("blockhash").unwrap_or(testhash);
    let blockraw = get_block_raw(&blockhash);

    println!("{}", blockraw);

    // println!("{}", std::str::from_utf8(&header));
}
