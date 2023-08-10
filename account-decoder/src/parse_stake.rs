use {
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError},
        StringAmount,
    },
    bincode::deserialize,
    solana_sdk::{
        clock::{Epoch, UnixTimestamp},
        stake::state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2},
    },
};

pub fn parse_stake(data: &[u8]) -> Result<StakeAccountType, ParseAccountError> {
    let stake_state: StakeStateV2 = deserialize(data)
        .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::Stake))?;
    let parsed_account = match stake_state {
        StakeStateV2::Uninitialized => StakeAccountType::Uninitialized,
        StakeStateV2::Initialized(meta) => StakeAccountType::Initialized(UiStakeAccount {
            meta: meta.into(),
            stake: None,
        }),
        StakeStateV2::Stake(meta, stake, _) => StakeAccountType::Delegated(UiStakeAccount {
            meta: meta.into(),
            stake: Some(stake.into()),
        }),
        StakeStateV2::RewardsPool => StakeAccountType::RewardsPool,
    };
    Ok(parsed_account)
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum StakeAccountType {
    Uninitialized,
    Initialized(UiStakeAccount),
    Delegated(UiStakeAccount),
    RewardsPool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStakeAccount {
    pub meta: UiMeta,
    pub stake: Option<UiStake>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMeta {
    pub rent_exempt_reserve: StringAmount,
    pub authorized: UiAuthorized,
    pub lockup: UiLockup,
}

impl From<Meta> for UiMeta {
    fn from(meta: Meta) -> Self {
        Self {
            rent_exempt_reserve: meta.rent_exempt_reserve.to_string(),
            authorized: meta.authorized.into(),
            lockup: meta.lockup.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiLockup {
    pub unix_timestamp: UnixTimestamp,
    pub epoch: Epoch,
    pub custodian: String,
}

impl From<Lockup> for UiLockup {
    fn from(lockup: Lockup) -> Self {
        Self {
            unix_timestamp: lockup.unix_timestamp,
            epoch: lockup.epoch,
            custodian: lockup.custodian.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiAuthorized {
    pub staker: String,
    pub withdrawer: String,
}

impl From<Authorized> for UiAuthorized {
    fn from(authorized: Authorized) -> Self {
        Self {
            staker: authorized.staker.to_string(),
            withdrawer: authorized.withdrawer.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStake {
    pub delegation: UiDelegation,
    pub credits_observed: u64,
}

impl From<Stake> for UiStake {
    fn from(stake: Stake) -> Self {
        Self {
            delegation: stake.delegation.into(),
            credits_observed: stake.credits_observed,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiDelegation {
    pub voter: String,
    pub stake: StringAmount,
    pub activation_epoch: StringAmount,
    pub deactivation_epoch: StringAmount,
    #[deprecated(
        since = "1.16.7",
        note = "Please use `solana_sdk::stake::stake::warmup_cooldown_rate()` instead"
    )]
    pub warmup_cooldown_rate: f64,
}

impl From<Delegation> for UiDelegation {
    fn from(delegation: Delegation) -> Self {
        #[allow(deprecated)]
        Self {
            voter: delegation.voter_pubkey.to_string(),
            stake: delegation.stake.to_string(),
            activation_epoch: delegation.activation_epoch.to_string(),
            deactivation_epoch: delegation.deactivation_epoch.to_string(),
            warmup_cooldown_rate: delegation.warmup_cooldown_rate,
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, bincode::serialize, solana_sdk::stake::stake_flags::StakeFlags};

    #[test]
    #[allow(deprecated)]
    fn test_parse_stake() {
        let stake_state = StakeStateV2::Uninitialized;
        let stake_data = serialize(&stake_state).unwrap();
        assert_eq!(
            parse_stake(&stake_data).unwrap(),
            StakeAccountType::Uninitialized
        );

        let pubkey = solana_sdk::pubkey::new_rand();
        let custodian = solana_sdk::pubkey::new_rand();
        let authorized = Authorized::auto(&pubkey);
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: 1,
            custodian,
        };
        let meta = Meta {
            rent_exempt_reserve: 42,
            authorized,
            lockup,
        };

        let stake_state = StakeStateV2::Initialized(meta);
        let stake_data = serialize(&stake_state).unwrap();
        assert_eq!(
            parse_stake(&stake_data).unwrap(),
            StakeAccountType::Initialized(UiStakeAccount {
                meta: UiMeta {
                    rent_exempt_reserve: 42.to_string(),
                    authorized: UiAuthorized {
                        staker: pubkey.to_string(),
                        withdrawer: pubkey.to_string(),
                    },
                    lockup: UiLockup {
                        unix_timestamp: 0,
                        epoch: 1,
                        custodian: custodian.to_string(),
                    }
                },
                stake: None,
            })
        );

        let voter_pubkey = solana_sdk::pubkey::new_rand();
        let stake = Stake {
            delegation: Delegation {
                voter_pubkey,
                stake: 20,
                activation_epoch: 2,
                deactivation_epoch: std::u64::MAX,
                warmup_cooldown_rate: 0.25,
            },
            credits_observed: 10,
        };

        let stake_state = StakeStateV2::Stake(meta, stake, StakeFlags::empty());
        let stake_data = serialize(&stake_state).unwrap();
        assert_eq!(
            parse_stake(&stake_data).unwrap(),
            StakeAccountType::Delegated(UiStakeAccount {
                meta: UiMeta {
                    rent_exempt_reserve: 42.to_string(),
                    authorized: UiAuthorized {
                        staker: pubkey.to_string(),
                        withdrawer: pubkey.to_string(),
                    },
                    lockup: UiLockup {
                        unix_timestamp: 0,
                        epoch: 1,
                        custodian: custodian.to_string(),
                    }
                },
                stake: Some(UiStake {
                    delegation: UiDelegation {
                        voter: voter_pubkey.to_string(),
                        stake: 20.to_string(),
                        activation_epoch: 2.to_string(),
                        deactivation_epoch: std::u64::MAX.to_string(),
                        warmup_cooldown_rate: 0.25,
                    },
                    credits_observed: 10,
                })
            })
        );

        let stake_state = StakeStateV2::RewardsPool;
        let stake_data = serialize(&stake_state).unwrap();
        assert_eq!(
            parse_stake(&stake_data).unwrap(),
            StakeAccountType::RewardsPool
        );

        let bad_data = vec![1, 2, 3, 4];
        assert!(parse_stake(&bad_data).is_err());
    }
}
