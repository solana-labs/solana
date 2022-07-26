use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg},
    log::*,
    solana_runtime::append_vec::AppendVec,
};

fn main() {
    solana_logger::setup_with_default("solana=info");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("file")
                .long("file")
                .takes_value(true)
                .value_name("<PATH>")
                .help("store to open"),
        )
        .arg(
            Arg::with_name("len")
                .long("len")
                .takes_value(true)
                .value_name("LEN")
                .help("len of store to open"),
        )
        .get_matches();

    let file = value_t_or_exit!(matches, "file", String);
    let len = value_t_or_exit!(matches, "len", usize);
    let (mut store, num_accounts) = AppendVec::new_from_file(file, len).expect("should succeed");
    store.set_no_remove_on_drop();
    info!(
        "store: len: {} capacity: {} accounts: {}",
        store.len(),
        store.capacity(),
        num_accounts,
    );
    for account in store.account_iter() {
        info!(
            "  account: {:?} version: {} data: {} hash: {:?}",
            account.meta.pubkey, account.meta.write_version, account.meta.data_len, account.hash
        );
    }
}

#[cfg(test)]
pub mod test {}
