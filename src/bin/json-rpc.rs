//! The `json-rpc` service launches an HTTP server to listen for
//! Solana rpc requests.

#[macro_use]
extern crate clap;
extern crate dirs;
extern crate jsonrpc_http_server;
extern crate solana;

use clap::{App, Arg};
use jsonrpc_http_server::jsonrpc_core::*;
use jsonrpc_http_server::*;
use solana::crdt::NodeInfo;
use solana::fullnode::Config;
use solana::rpc::*;
use std::error;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::result::Result;

fn main() -> Result<(), Box<error::Error>> {
    let matches = App::new("json-rpc")
        .version(crate_version!())
        .arg(
            Arg::with_name("leader")
                .short("l")
                .long("leader")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/leader.json"),
        )
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("/path/to/id.json"),
        )
        .get_matches();
    let leader: NodeInfo;
    if let Some(l) = matches.value_of("leader") {
        leader = read_leader(l).node_info;
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        leader = NodeInfo::new_leader(&server_addr);
    };

    let id_path: String = matches
        .value_of("keypair")
        .expect("Unable to parse keypair path")
        .to_string();

    let mut io = MetaIoHandler::default();
    let rpc = RpcSolImpl;
    io.extend_with(rpc.to_delegate());

    let rpc_addr = format!("0.0.0.0:{}", RPC_PORT);
    let server = ServerBuilder::new(io)
        .meta_extractor(move |_req: &hyper::Request| Meta {
            leader: Some(leader.clone()),
            keypair_location: Some(id_path.clone()),
        })
        .threads(4)
        .cors(DomainsValidation::AllowOnly(vec![
            AccessControlAllowOrigin::Any,
        ]))
        .start_http(
            &rpc_addr
                .parse()
                .expect("Unable to parse RPC server address"),
        )?;

    server.wait();
    Ok(())
}

fn read_leader(path: &str) -> Config {
    let file = File::open(path).unwrap_or_else(|_| panic!("file not found: {}", path));
    serde_json::from_reader(file).unwrap_or_else(|_| panic!("failed to parse {}", path))
}
