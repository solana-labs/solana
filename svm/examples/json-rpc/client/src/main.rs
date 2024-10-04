use clap::{crate_description, crate_name, crate_version, App, Arg};

mod client;
mod utils;

fn main() {
    let version = crate_version!().to_string();
    let args = std::env::args().collect::<Vec<_>>();
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(version.as_str())
        .arg(
            Arg::with_name("config")
                .long("config")
                .short("C")
                .takes_value(true)
                .value_name("CONFIG")
                .help("Config filepath"),
        )
        .arg(
            Arg::with_name("keypair")
                .long("keypair")
                .short("k")
                .takes_value(true)
                .value_name("KEYPAIR")
                .help("Filepath or URL to a keypair"),
        )
        .arg(
            Arg::with_name("url")
                .long("url")
                .short("u")
                .takes_value(true)
                .value_name("URL_OR_MONIKER")
                .help("URL for JSON RPC Server"),
        )
        .get_matches_from(args);
    let config = matches.value_of("config");
    let keypair = matches.value_of("keypair").unwrap();
    let url = matches.value_of("url");
    let connection = client::establish_connection(&url, &config).unwrap();
    println!(
        "Connected to Simulation server running version ({}).",
        connection.get_version().unwrap()
    );
    let player = utils::get_player(&config).unwrap();
    let program = client::get_program(keypair, &connection).unwrap();
    client::say_hello(&player, &program, &connection).unwrap();
}
