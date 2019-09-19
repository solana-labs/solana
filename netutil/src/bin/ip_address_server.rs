use clap::{crate_version, App, Arg};
use tokio::net::TcpListener;

fn main() {
    solana_logger::setup();
    let matches = App::new("solana-ip-address-server")
        .version(crate_version!())
        .arg(
            Arg::with_name("port")
                .index(1)
                .required(true)
                .help("TCP port to bind to"),
        )
        .get_matches();

    let port = matches.value_of("port").unwrap();
    let port = port
        .parse()
        .unwrap_or_else(|_| panic!("Unable to parse {}", port));
    let tcp_listener = TcpListener::bind(&port).expect("unable to start tcp listener");
    let _runtime = solana_netutil::ip_echo_server(tcp_listener);
    loop {
        std::thread::park();
    }
}
