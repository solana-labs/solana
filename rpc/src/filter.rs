use {
    solana_inline_spl::{token::GenericTokenAccount, token_2022::Account},
    solana_rpc_client_api::filter::RpcFilterType,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
};

pub fn filter_allows(filter: &RpcFilterType, account: &AccountSharedData) -> bool {
    match filter {
        RpcFilterType::DataSize(size) => account.data().len() as u64 == *size,
        RpcFilterType::Memcmp(compare) => compare.bytes_match(account.data()),
        RpcFilterType::TokenAccountState => Account::valid_account_data(account.data()),
    }
}
