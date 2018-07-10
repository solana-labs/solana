extern crate clap;
extern crate serde_json;
extern crate solana;

use clap::{App, Arg};
use solana::crdt::{get_ip_addr, parse_port_or_addr, ReplicatedData};
use solana::nat::get_public_ip_addr;
use std::io;
use std::net::SocketAddr;

fn main() {
    let matches = App::new("fullnode-config")
        .arg(
            Arg::with_name("local")
                .short("l")
                .long("local")
                .takes_value(false)
                .help("detect network address from local machine configuration"),
        )
        .arg(
            Arg::with_name("public")
                .short("p")
                .long("public")
                .takes_value(false)
                .help("detect public network address using public servers"),
        )
        .arg(
            Arg::with_name("bind")
                .short("b")
                .long("bind")
                .value_name("PORT")
                .takes_value(true)
                .help("bind to port or address"),
        )
        .get_matches();

    let bind_addr: SocketAddr = {
        let mut bind_addr = parse_port_or_addr({
            if let Some(b) = matches.value_of("bind") {
                Some(b.to_string())
            } else {
                None
            }
        });
        if matches.is_present("local") {
            let ip = get_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        if matches.is_present("public") {
            let ip = get_public_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        bind_addr
    };

    // we need all the receiving sockets to be bound within the expected
    // port range that we open on aws
    let repl_data = ReplicatedData::new_leader(&bind_addr);
    let stdout = io::stdout();
    serde_json::to_writer(stdout, &repl_data).expect("serialize");
}
