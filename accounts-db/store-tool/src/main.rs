use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg},
    solana_accounts_db::append_vec::AppendVec,
    solana_sdk::{account::ReadableAccount, system_instruction::MAX_PERMITTED_DATA_LENGTH},
    std::{mem::ManuallyDrop, num::Saturating},
};

fn main() {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("file")
                .index(1)
                .takes_value(true)
                .value_name("PATH")
                .help("Account storage file to open"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .takes_value(false)
                .help("Show additional account information"),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let file = value_t_or_exit!(matches, "file", String);
    let store = AppendVec::new_for_store_tool(&file).unwrap_or_else(|err| {
        eprintln!("failed to open storage file '{file}': {err}");
        std::process::exit(1);
    });
    // By default, when the AppendVec is dropped, the backing file will be removed.
    // We do not want to remove the backing file here in the store-tool, so prevent dropping.
    let store = ManuallyDrop::new(store);

    // max data size is 10 MiB (10,485,760 bytes)
    // therefore, the max width is ceil(log(10485760))
    let data_size_width = (MAX_PERMITTED_DATA_LENGTH as f64).log10().ceil() as usize;
    let offset_width = (store.capacity() as f64).log(16.0).ceil() as usize;

    let mut num_accounts = Saturating(0usize);
    let mut stored_accounts_size = Saturating(0);
    store.scan_accounts(|account| {
        if verbose {
            println!("{account:?}");
        } else {
            println!(
                "{:#0offset_width$x}: {:44}, owner: {:44}, data size: {:data_size_width$}, lamports: {}",
                account.offset(),
                account.pubkey().to_string(),
                account.owner().to_string(),
                account.data_len(),
                account.lamports(),
            );
        }
        num_accounts += 1;
        stored_accounts_size += account.stored_size();
    });

    println!(
        "number of accounts: {}, stored accounts size: {}, file size: {}",
        num_accounts,
        stored_accounts_size,
        store.capacity(),
    );
}
