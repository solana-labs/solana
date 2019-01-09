use solana_sdk::signature::read_pkcs8;
use std::io;

fn main() {
    let matches = clap::App::new("fullnode-config")
        .version(clap::crate_version!())
        .arg(
            clap::Arg::with_name("local")
                .short("l")
                .long("local")
                .takes_value(false)
                .help("Detect network address from local machine configuration"),
        )
        .arg(
            clap::Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        )
        .arg(
            clap::Arg::with_name("public")
                .short("p")
                .long("public")
                .takes_value(false)
                .help("Detect public network address using public servers"),
        )
        .arg(
            clap::Arg::with_name("bind")
                .short("b")
                .long("bind")
                .value_name("PORT")
                .takes_value(true)
                .help("Bind to port or address"),
        )
        .get_matches();

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        path.to_str().unwrap()
    };

    let config = solana_fullnode_config::Config {
        bind_port_or_address: matches.value_of("bind").map(|s| s.to_string()),
        use_local_address: matches.is_present("local"),
        use_public_address: matches.is_present("public"),
        identity_pkcs8: read_pkcs8(id_path).expect("invalid keypair"),
    };

    let stdout = io::stdout();
    serde_json::to_writer(stdout, &config).expect("serialize");
}
