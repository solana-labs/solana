/// The interface for AccountsDb plugins. A plugin must implement
/// the AccountsDbPlugin trait to work with the Solana Validator.
/// In addition the dynamic libraray must export a "C" function _create_plugin which
/// creates the implementation of the plugin.
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, PartialEq)]
pub struct ReplicaAccountMeta {
    pub pubkey: Pubkey,
    pub lamports: u64,
    pub owner: Pubkey,
    pub executable: bool,
    pub rent_epoch: u64,
}

#[derive(Clone, PartialEq)]
pub struct ReplicaAccountInfo {
    pub account_meta: ReplicaAccountMeta,
    pub hash: Vec<u8>,
    pub data: Vec<u8>,
}

pub enum AccountsDbPluginError {
    AccountsUpdateError,
}

pub type Result<T> = std::result::Result<T, AccountsDbPluginError>;

pub trait AccountsDbPlugin {
    fn name(&self) -> &'static str;

    /// The callback called when a plugin is loaded by the system
    /// Used for doing whatever initialization by the plugin
    /// The _config_file points to the file name contains the name of the
    /// of the config file. The framework does not stipulate the format of the
    /// file -- it is totoally up to the plugin implementation.
    fn on_load(&self, _config_file: &str) {}

    /// The callback called right before a plugin is unloaded by the system
    /// Used for doing cleanup before being unloaded.
    fn on_unload(&self) {}

    /// Called when an account is updated at a slot.
    fn update_account(&self, account: ReplicaAccountInfo, slot: u64) -> Result<()>;
}
