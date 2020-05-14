use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
#[allow(clippy::large_enum_variant)]
pub enum StakeState {
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake),
    RewardsPool,
}

#[wasm_bindgen(js_name = StakeState)]
#[derive(Copy, Clone)]
pub enum State {
    Uninitialized,
    Initialized,
    Delegated,
    RewardsPool,
}

#[wasm_bindgen]
pub struct StakeAccount {
    pub meta: Option<Meta>,
    pub stake: Option<Stake>,
    pub state: State,
}

impl From<StakeState> for StakeAccount {
    fn from(state: StakeState) -> Self {
        match state {
            StakeState::Uninitialized => StakeAccount {
                state: State::Uninitialized,
                meta: None,
                stake: None,
            },
            StakeState::Initialized(meta) => StakeAccount {
                state: State::Initialized,
                meta: Some(meta),
                stake: None,
            },
            StakeState::Stake(meta, stake) => StakeAccount {
                state: State::Delegated,
                meta: Some(meta),
                stake: Some(stake),
            },
            StakeState::RewardsPool => StakeAccount {
                state: State::RewardsPool,
                meta: None,
                stake: None,
            },
        }
    }
}

#[wasm_bindgen]
impl StakeAccount {
    #[wasm_bindgen(js_name = fromAccountData)]
    pub fn from_account_data(data: &[u8]) -> Result<StakeAccount, JsValue> {
        let stake_state: StakeState = bincode::deserialize(data)
            .map_err(|_| JsValue::from_str("invalid stake account data"))?;
        return Ok(stake_state.into());
    }

    #[wasm_bindgen(js_name = displayState)]
    pub fn display_state(&self) -> String {
        match self.state {
            State::Uninitialized => "Uninitialized".to_string(),
            State::Initialized => "Initialized".to_string(),
            State::Delegated => "Delegated".to_string(),
            State::RewardsPool => "RewardsPool".to_string(),
        }
    }
}

/// UnixTimestamp is an approximate measure of real-world time,
/// expressed as Unix time (ie. seconds since the Unix epoch)
pub type UnixTimestamp = i64;

#[wasm_bindgen]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, or
    ///   changes to authorized staker or withdrawer, unless the
    ///   transaction is signed by the custodian
    unix_timestamp: UnixTimestamp,
    /// epoch height at which this stake will allow withdrawal, or
    ///   changes to authorized staker or withdrawer, unless the
    ///   transaction is signed by the custodian
    ///  to the custodian
    epoch: Epoch,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: Pubkey,
}

#[wasm_bindgen]
impl Lockup {
    #[wasm_bindgen(getter = unixTimestamp)]
    pub fn unix_timestamp(&self) -> f64 {
        self.unix_timestamp as f64
    }

    #[wasm_bindgen(getter)]
    pub fn epoch(&self) -> f64 {
        self.epoch as f64
    }
}

/// Epoch is a unit of time a given leader schedule is honored,
///  some number of Slots.
pub type Epoch = u64;

#[wasm_bindgen]
#[repr(transparent)]
#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey([u8; 32]);

#[wasm_bindgen]
impl Pubkey {
    #[wasm_bindgen(js_name = toBase58)]
    pub fn to_base_58(&self) -> String {
        bs58::encode(&self.0).into_string()
    }
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Authorized {
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Meta {
    rent_exempt_reserve: u64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

#[wasm_bindgen]
impl Meta {
    #[wasm_bindgen(getter = rentExemptReserve)]
    pub fn rent_exempt_reserve(&self) -> f64 {
        self.rent_exempt_reserve as f64
    }
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    credits_observed: u64,
}

#[wasm_bindgen]
impl Stake {
    #[wasm_bindgen(getter = creditsObserved)]
    pub fn credits_observed(&self) -> f64 {
        self.credits_observed as f64
    }
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Delegation {
    /// to whom the stake is delegated
    voter_pubkey: Pubkey,
    /// activated stake amount, set at delegate() time
    stake: u64,
    /// epoch at which this stake was activated, std::Epoch::MAX if is a bootstrap stake
    activation_epoch: Epoch,
    /// epoch the stake was deactivated, std::Epoch::MAX if not deactivated
    deactivation_epoch: Epoch,
    /// how much stake we can activate per-epoch as a fraction of currently effective stake
    warmup_cooldown_rate: f64,
}

#[wasm_bindgen]
impl Delegation {
    #[wasm_bindgen(getter = voterPubkey)]
    pub fn voter_pubkey(&self) -> Pubkey {
        self.voter_pubkey
    }

    #[wasm_bindgen(getter)]
    pub fn stake(&self) -> f64 {
        self.stake as f64
    }

    #[wasm_bindgen(js_name = isBootstrapStake)]
    pub fn is_bootstrap_stake(&self) -> bool {
        self.activation_epoch == Epoch::MAX
    }

    #[wasm_bindgen(js_name = isDeactivated)]
    pub fn is_deactivated(&self) -> bool {
        self.deactivation_epoch != Epoch::MAX
    }

    #[wasm_bindgen(getter = activationEpoch)]
    pub fn activation_epoch(&self) -> f64 {
        self.activation_epoch as f64
    }

    #[wasm_bindgen(getter = deactivationEpoch)]
    pub fn deactivation_epoch(&self) -> f64 {
        self.deactivation_epoch as f64
    }

    #[wasm_bindgen(getter = warmupCooldownRate)]
    pub fn warmup_cooldown_rate(&self) -> f64 {
        self.warmup_cooldown_rate
    }
}
