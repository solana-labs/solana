use {
    clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg},
    log::*,
    solana_runtime::{account_storage::meta::StoredAccountMeta, append_vec::AppendVec},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        hash::Hash,
        pubkey::Pubkey,
    },
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
    let len = value_t!(matches, "len", usize)
        .unwrap_or_else(|_| std::fs::metadata(&file).unwrap().len() as usize);

    let mut store = AppendVec::new_from_file_unchecked(file, len).expect("should succeed");
    store.set_no_remove_on_drop();
    info!("store: len: {} capacity: {}", store.len(), store.capacity());
    let mut num_accounts: usize = 0;
    let mut stored_accounts_len: usize = 0;
    for account in store.account_iter() {
        if is_account_zeroed(&account) {
            break;
        }
        info!(
            "  account: {:?} version: {} lamports: {} data: {} hash: {:?}",
            account.pubkey(),
            account.write_version(),
            account.lamports(),
            account.data_len(),
            account.hash()
        );
        num_accounts = num_accounts.saturating_add(1);
        stored_accounts_len = stored_accounts_len.saturating_add(account.stored_size());
    }
    info!(
        "num_accounts: {} stored_accounts_len: {}",
        num_accounts, stored_accounts_len
    );
}

fn is_account_zeroed(account: &StoredAccountMeta) -> bool {
    account.hash() == &Hash::default()
        && account.data_len() == 0
        && account.write_version() == 0
        && account.pubkey() == &Pubkey::default()
        && account.clone_account() == AccountSharedData::default()
}

#[cfg(test)]
pub mod test {}
