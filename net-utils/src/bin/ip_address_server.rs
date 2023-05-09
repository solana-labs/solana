use {
    clap::{Arg, Command},
    std::net::{Ipv4Addr, SocketAddr, TcpListener},
};

fn main() {
    solana_logger::setup();
    let matches = Command::new("solana-ip-address-server")
        .version(solana_version::version!())
        .arg(
            Arg::new("port")
                .index(1)
                .required(true)
                .help("TCP port to bind to"),
        )
        .get_matches();

    let port = matches.value_of("port").unwrap();
    let port = port
        .parse()
        .unwrap_or_else(|_| panic!("Unable to parse {port}"));
    let bind_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let tcp_listener = TcpListener::bind(bind_addr).expect("unable to start tcp listener");
    let _runtime = solana_net_utils::ip_echo_server(tcp_listener, /*shred_version=*/ None);
    loop {
        std::thread::park();
    }
}
