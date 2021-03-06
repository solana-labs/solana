use crate::account::AccountSharedData;
use crate::account::AnAccount;
use crate::account::AnAccountWritable;
pub use solana_program::feature::*;

pub fn from_account<T: AnAccount>(account: &T) -> Option<Feature> {
    if account.owner() != &id() {
        None
    } else {
        bincode::deserialize(account.data()).ok()
    }
}

pub fn to_account<T: AnAccountWritable>(feature: &Feature, account: &mut T) -> Option<()> {
    bincode::serialize_into(account.data_as_mut_slice(), feature).ok()
}

pub fn create_account(feature: &Feature, lamports: u64) -> AccountSharedData {
    let data_len = Feature::size_of().max(bincode::serialized_size(feature).unwrap() as usize);
    let mut account = AccountSharedData::new(lamports, data_len, &id());
    to_account(feature, &mut account).unwrap();
    account
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn feature_deserialize_none() {
        let just_initialized = AccountSharedData::new(42, Feature::size_of(), &id());
        assert_eq!(
            from_account(&just_initialized),
            Some(Feature { activated_at: None })
        );
    }
}
