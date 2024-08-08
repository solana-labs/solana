use {
    super::{StakeAccount, Stakes, StakesEnum},
    crate::stake_history::StakeHistory,
    im::HashMap as ImHashMap,
    serde::{ser::SerializeMap, Serialize, Serializer},
    solana_sdk::{clock::Epoch, pubkey::Pubkey, stake::state::Delegation},
    solana_stake_program::stake_state::Stake,
    solana_vote::vote_account::VoteAccounts,
    std::sync::Arc,
};

// In order to maintain backward compatibility, the StakesEnum in EpochStakes
// and SerializableVersionedBank should be serialized as Stakes<Delegation>.
pub(crate) mod serde_stakes_enum_compat {
    use {
        super::*,
        serde::{Deserialize, Deserializer, Serialize, Serializer},
    };

    pub(crate) fn serialize<S>(stakes: &StakesEnum, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match stakes {
            StakesEnum::Delegations(stakes) => stakes.serialize(serializer),
            StakesEnum::Stakes(stakes) => serialize_stakes_as_delegations(stakes, serializer),
            StakesEnum::Accounts(stakes) => {
                serialize_stake_accounts_as_delegations(stakes, serializer)
            }
        }
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Arc<StakesEnum>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let stakes = Stakes::<Delegation>::deserialize(deserializer)?;
        Ok(Arc::new(StakesEnum::Delegations(stakes)))
    }
}

fn serialize_stakes_as_delegations<S: Serializer>(
    stakes: &Stakes<Stake>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    SerdeStakeVariantStakes::from(stakes.clone()).serialize(serializer)
}

fn serialize_stake_accounts_as_delegations<S: Serializer>(
    stakes: &Stakes<StakeAccount>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    SerdeStakeAccountVariantStakes::from(stakes.clone()).serialize(serializer)
}

impl From<Stakes<Stake>> for SerdeStakeVariantStakes {
    fn from(stakes: Stakes<Stake>) -> Self {
        let Stakes {
            vote_accounts,
            stake_delegations,
            unused,
            epoch,
            stake_history,
        } = stakes;

        Self {
            vote_accounts,
            stake_delegations: SerdeStakeMapWrapper(stake_delegations),
            unused,
            epoch,
            stake_history,
        }
    }
}

impl From<Stakes<StakeAccount>> for SerdeStakeAccountVariantStakes {
    fn from(stakes: Stakes<StakeAccount>) -> Self {
        let Stakes {
            vote_accounts,
            stake_delegations,
            unused,
            epoch,
            stake_history,
        } = stakes;

        Self {
            vote_accounts,
            stake_delegations: SerdeStakeAccountMapWrapper(stake_delegations),
            unused,
            epoch,
            stake_history,
        }
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize)]
struct SerdeStakeVariantStakes {
    vote_accounts: VoteAccounts,
    stake_delegations: SerdeStakeMapWrapper,
    unused: u64,
    epoch: Epoch,
    stake_history: StakeHistory,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Serialize)]
struct SerdeStakeAccountVariantStakes {
    vote_accounts: VoteAccounts,
    stake_delegations: SerdeStakeAccountMapWrapper,
    unused: u64,
    epoch: Epoch,
    stake_history: StakeHistory,
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
struct SerdeStakeMapWrapper(ImHashMap<Pubkey, Stake>);
impl Serialize for SerdeStakeMapWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_map(Some(self.0.len()))?;
        for (pubkey, stake) in self.0.iter() {
            s.serialize_entry(pubkey, &stake.delegation)?;
        }
        s.end()
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
struct SerdeStakeAccountMapWrapper(ImHashMap<Pubkey, StakeAccount>);
impl Serialize for SerdeStakeAccountMapWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_map(Some(self.0.len()))?;
        for (pubkey, stake_account) in self.0.iter() {
            s.serialize_entry(pubkey, stake_account.delegation())?;
        }
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*, crate::stakes::StakesCache, rand::Rng, solana_sdk::rent::Rent,
        solana_stake_program::stake_state, solana_vote_program::vote_state,
    };

    #[test]
    fn test_serde_stakes_enum_compat() {
        #[derive(Debug, PartialEq, Deserialize, Serialize)]
        struct Dummy {
            head: String,
            #[serde(with = "serde_stakes_enum_compat")]
            stakes: Arc<StakesEnum>,
            tail: String,
        }
        let mut rng = rand::thread_rng();
        let stakes_cache = StakesCache::new(Stakes {
            unused: rng.gen(),
            epoch: rng.gen(),
            ..Stakes::default()
        });
        for _ in 0..rng.gen_range(5usize..10) {
            let vote_pubkey = solana_sdk::pubkey::new_rand();
            let vote_account = vote_state::create_account(
                &vote_pubkey,
                &solana_sdk::pubkey::new_rand(), // node_pubkey
                rng.gen_range(0..101),           // commission
                rng.gen_range(0..1_000_000),     // lamports
            );
            stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
            for _ in 0..rng.gen_range(10usize..20) {
                let stake_pubkey = solana_sdk::pubkey::new_rand();
                let rent = Rent::with_slots_per_epoch(rng.gen());
                let stake_account = stake_state::create_account(
                    &stake_pubkey, // authorized
                    &vote_pubkey,
                    &vote_account,
                    &rent,
                    rng.gen_range(0..1_000_000), // lamports
                );
                stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            }
        }
        let stakes: Stakes<StakeAccount> = stakes_cache.stakes().clone();
        assert!(stakes.vote_accounts.as_ref().len() >= 5);
        assert!(stakes.stake_delegations.len() >= 50);
        let dummy = Dummy {
            head: String::from("dummy-head"),
            stakes: Arc::new(StakesEnum::from(stakes.clone())),
            tail: String::from("dummy-tail"),
        };
        assert!(dummy.stakes.vote_accounts().as_ref().len() >= 5);
        let data = bincode::serialize(&dummy).unwrap();
        let other: Dummy = bincode::deserialize(&data).unwrap();
        assert_eq!(other, dummy);
        let stakes = Stakes::<Delegation>::from(stakes);
        assert!(stakes.vote_accounts.as_ref().len() >= 5);
        assert!(stakes.stake_delegations.len() >= 50);
        let other = match &*other.stakes {
            StakesEnum::Accounts(_) | StakesEnum::Stakes(_) => panic!("wrong type!"),
            StakesEnum::Delegations(delegations) => delegations,
        };
        assert_eq!(other, &stakes)
    }
}
