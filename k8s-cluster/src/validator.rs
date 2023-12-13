use {
    crate::ValidatorType,
    solana_sdk::keypair::Keypair,
};



pub struct Validator {

    validator_type: ValidatorType,
    validator_sol: f64,
    stake_in_sol: f64,

    //Docker config
    docker_base_image: String,
    docker_container_name: String,

    // Kubernetes pod labels
    validator_labels: BTreeMap<String, String>,

    //Accounts
    identity_path: PathBuf,
    idendity: Keypair,
    vote_path: PathBuf,
    vote_account: Keypair,
    stake_path: PathBuf,
    stake_account: Keypair,

    
}