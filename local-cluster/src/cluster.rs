use safecoin_client::thin_client::ThinClient;
use safecoin_core::contact_info::ContactInfo;
use safecoin_core::validator::Validator;
use safecoin_core::validator::ValidatorConfig;
use safecoin_sdk::pubkey::Pubkey;
use safecoin_sdk::signature::Keypair;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ValidatorInfo {
    pub keypair: Arc<Keypair>,
    pub voting_keypair: Arc<Keypair>,
    pub ledger_path: PathBuf,
    pub contact_info: ContactInfo,
}

pub struct ClusterValidatorInfo {
    pub info: ValidatorInfo,
    pub config: ValidatorConfig,
    pub validator: Option<Validator>,
}

impl ClusterValidatorInfo {
    pub fn new(
        validator_info: ValidatorInfo,
        config: ValidatorConfig,
        validator: Validator,
    ) -> Self {
        Self {
            info: validator_info,
            config,
            validator: Some(validator),
        }
    }
}

pub trait Cluster {
    fn get_node_pubkeys(&self) -> Vec<Pubkey>;
    fn get_validator_client(&self, pubkey: &Pubkey) -> Option<ThinClient>;
    fn get_contact_info(&self, pubkey: &Pubkey) -> Option<&ContactInfo>;
    fn exit_node(&mut self, pubkey: &Pubkey) -> ClusterValidatorInfo;
    fn restart_node(&mut self, pubkey: &Pubkey, cluster_validator_info: ClusterValidatorInfo);
    fn create_restart_context(
        &mut self,
        pubkey: &Pubkey,
        cluster_validator_info: &mut ClusterValidatorInfo,
    ) -> (safecoin_core::cluster_info::Node, Option<ContactInfo>);
    fn restart_node_with_context(
        cluster_validator_info: ClusterValidatorInfo,
        restart_context: (safecoin_core::cluster_info::Node, Option<ContactInfo>),
    ) -> ClusterValidatorInfo;
    fn add_node(&mut self, pubkey: &Pubkey, cluster_validator_info: ClusterValidatorInfo);
    fn exit_restart_node(&mut self, pubkey: &Pubkey, config: ValidatorConfig);
}
