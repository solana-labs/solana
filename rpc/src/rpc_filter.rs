use {
    solana_client::rpc_filter::RpcFilterType,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
    spl_token_2022::{generic_token_account::GenericTokenAccount, state::Account},
};

pub fn satisfies_filter(account: &AccountSharedData, filter_type: &RpcFilterType) -> bool {
    match filter_type {
        RpcFilterType::DataSize(size) => account.data().len() as u64 == *size,
        RpcFilterType::Memcmp(compare) => compare.bytes_match(account.data()),
        RpcFilterType::TokenAccountState => Account::valid_account_data(account.data()),
    }
}
