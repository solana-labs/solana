use {
    crate::stake_rewards::RewardInfo,
    solana_sdk::{pubkey::Pubkey, reward_type::RewardType},
    std::collections::HashMap,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RentDebit {
    rent_collected: u64,
    post_balance: u64,
}

impl RentDebit {
    fn try_into_reward_info(self) -> Option<RewardInfo> {
        let rent_debit = i64::try_from(self.rent_collected)
            .ok()
            .and_then(|r| r.checked_neg());
        rent_debit.map(|rent_debit| RewardInfo {
            reward_type: RewardType::Rent,
            lamports: rent_debit,
            post_balance: self.post_balance,
            commission: None, // Not applicable
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RentDebits(HashMap<Pubkey, RentDebit>);
impl RentDebits {
    pub fn get_account_rent_debit(&self, address: &Pubkey) -> u64 {
        self.0
            .get(address)
            .map(|r| r.rent_collected)
            .unwrap_or_default()
    }

    // These functions/fields are only usable from a dev context (i.e. tests and benches)
    #[cfg(feature = "dev-context-only-utils")]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(feature = "dev-context-only-utils")]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn insert(&mut self, address: &Pubkey, rent_collected: u64, post_balance: u64) {
        if rent_collected != 0 {
            self.0.insert(
                *address,
                RentDebit {
                    rent_collected,
                    post_balance,
                },
            );
        }
    }

    pub fn into_unordered_rewards_iter(self) -> impl Iterator<Item = (Pubkey, RewardInfo)> {
        self.0
            .into_iter()
            .filter_map(|(address, rent_debit)| Some((address, rent_debit.try_into_reward_info()?)))
    }
}
