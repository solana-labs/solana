extern crate getopts;
extern crate serde_json;
extern crate solana;

use getopts::Options;
use solana::crdt::{get_ip_addr, parse_port_or_addr, ReplicatedData};
use solana::nat::get_public_ip_addr;
use std::env;
use std::io;
use std::net::SocketAddr;
use std::process::exit;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: {} [options]\n\n", program);
    brief += "  Create a solana fullnode config file\n";

    print!("{}", opts.usage(&brief));
}

fn main() {
    let mut opts = Options::new();
    opts.optopt("b", "", "bind", "bind to port or address");
    opts.optflag(
        "p",
        "",
        "detect public network address using public servers",
    );
    opts.optflag(
        "l",
        "",
        "detect network address from local machine configuration",
    );
    opts.optflag("h", "help", "print help");
    let args: Vec<String> = env::args().collect();
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    if matches.opt_present("h") {
        let program = args[0].clone();
        print_usage(&program, opts);
        return;
    }

    let bind_addr: SocketAddr = {
        let mut bind_addr = parse_port_or_addr(matches.opt_str("b"));
        if matches.opt_present("l") {
            let ip = get_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        if matches.opt_present("p") {
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
