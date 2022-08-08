use {
    clap::Parser,
    log::*,
    solana_runtime::append_vec::{AppendVec, StoredAccountMeta},
    solana_sdk::{account::AccountSharedData, hash::Hash, pubkey::Pubkey},
};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Storage file to read
    #[clap(short, long, value_parser)]
    file: String,
    /// Length of accounts written to the storage file
    #[clap(short, long, value_parser)]
    len: Option<usize>,
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    let args = Args::parse();

    let file = args.file;
    let len = args
        .len
        .unwrap_or_else(|| std::fs::metadata(&file).unwrap().len() as usize);

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
            "  account: {:?} version: {} data: {} hash: {:?}",
            account.meta.pubkey, account.meta.write_version, account.meta.data_len, account.hash
        );
        num_accounts = num_accounts.saturating_add(1);
        stored_accounts_len = stored_accounts_len.saturating_add(account.stored_size);
    }
    info!(
        "num_accounts: {} stored_accounts_len: {}",
        num_accounts, stored_accounts_len
    );
}

fn is_account_zeroed(account: &StoredAccountMeta) -> bool {
    account.hash == &Hash::default()
        && account.meta.data_len == 0
        && account.meta.write_version == 0
        && account.meta.pubkey == Pubkey::default()
        && account.clone_account() == AccountSharedData::default()
}

#[cfg(test)]
pub mod test {}
