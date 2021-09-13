/// The interface for AccountsDb plugins. A plugin must implement
/// the AccountsDbPlugin trait to work with the Solana Validator.
/// In addition the dynamic libraray must export a "C" function _create_plugin which
/// creates the implementation of the plugin.

#[derive(Clone, PartialEq, Debug)]
pub struct ReplicaAccountMeta {
    pub pubkey: Vec<u8>,
    pub lamports: u64,
    pub owner: Vec<u8>,
    pub executable: bool,
    pub rent_epoch: u64,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ReplicaAccountInfo {
    pub account_meta: ReplicaAccountMeta,
    pub hash: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum AccountsDbPluginError {
    AccountsUpdateError{ msg: String},
}


impl std::error::Error for AccountsDbPluginError {
}

impl std::fmt::Display for AccountsDbPluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            AccountsDbPluginError::AccountsUpdateError { ref msg } => write!(f, "{}", msg),
        }
    }
}


pub type Result<T> = std::result::Result<T, AccountsDbPluginError>;

pub trait AccountsDbPlugin {
    fn name(&self) -> &'static str;

    /// The callback called when a plugin is loaded by the system
    /// Used for doing whatever initialization by the plugin
    /// The _config_file points to the file name contains the name of the
    /// of the config file. The framework does not stipulate the format of the
    /// file -- it is totoally up to the plugin implementation.
    fn on_load(&mut self, _config_file: &str) -> Result<()> {Ok(())}

    /// The callback called right before a plugin is unloaded by the system
    /// Used for doing cleanup before being unloaded.
    fn on_unload(&mut self) {}

    /// Called when an account is updated at a slot.
    fn update_account(&self, account: ReplicaAccountInfo, slot: u64) -> Result<()>;
}