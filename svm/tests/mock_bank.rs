use {
    solana_program_runtime::loaded_programs::{BlockRelation, ForkGraph},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        clock::Epoch,
        feature_set::FeatureSet,
        native_loader,
        pubkey::Pubkey,
        slot_hashes::Slot,
    },
    solana_svm::transaction_processing_callback::TransactionProcessingCallback,
    solana_type_overrides::sync::{Arc, RwLock},
    std::{
        cmp::Ordering,
        collections::HashMap,
        env,
        fs::{self, File},
        io::Read,
    },
};

pub struct MockForkGraph {}

impl ForkGraph for MockForkGraph {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        match a.cmp(&b) {
            Ordering::Less => BlockRelation::Ancestor,
            Ordering::Equal => BlockRelation::Equal,
            Ordering::Greater => BlockRelation::Descendant,
        }
    }

    fn slot_epoch(&self, _slot: Slot) -> Option<Epoch> {
        Some(0)
    }
}

#[derive(Default, Clone)]
pub struct MockBankCallback {
    pub feature_set: Arc<FeatureSet>,
    pub account_shared_data: Arc<RwLock<HashMap<Pubkey, AccountSharedData>>>,
}

impl TransactionProcessingCallback for MockBankCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        if let Some(data) = self.account_shared_data.read().unwrap().get(account) {
            if data.lamports() == 0 {
                None
            } else {
                owners.iter().position(|entry| data.owner() == entry)
            }
        } else {
            None
        }
    }

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.account_shared_data
            .read()
            .unwrap()
            .get(pubkey)
            .cloned()
    }

    fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
        let account_data = native_loader::create_loadable_account_with_fields(name, (5000, 0));

        self.account_shared_data
            .write()
            .unwrap()
            .insert(*program_id, account_data);
    }
}

impl MockBankCallback {
    #[allow(unused)]
    pub fn override_feature_set(&mut self, new_set: FeatureSet) {
        self.feature_set = Arc::new(new_set)
    }
}

#[allow(unused)]
fn load_program(name: String) -> Vec<u8> {
    // Loading the program file
    let mut dir = env::current_dir().unwrap();
    dir.push("tests");
    dir.push("example-programs");
    dir.push(name.as_str());
    let name = name.replace('-', "_");
    dir.push(name + "_program.so");
    let mut file = File::open(dir.clone()).expect("file not found");
    let metadata = fs::metadata(dir).expect("Unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    file.read_exact(&mut buffer).expect("Buffer overflow");
    buffer
}

#[allow(unused)]
pub fn deploy_program(
    name: String,
    deployment_slot: Slot,
    mock_bank: &mut MockBankCallback,
) -> Pubkey {
    let program_account = Pubkey::new_unique();
    let program_data_account = Pubkey::new_unique();
    let state = UpgradeableLoaderState::Program {
        programdata_address: program_data_account,
    };

    // The program account must have funds and hold the executable binary
    let mut account_data = AccountSharedData::default();
    account_data.set_data(bincode::serialize(&state).unwrap());
    account_data.set_lamports(25);
    account_data.set_owner(bpf_loader_upgradeable::id());
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(program_account, account_data);

    let mut account_data = AccountSharedData::default();
    let state = UpgradeableLoaderState::ProgramData {
        slot: deployment_slot,
        upgrade_authority_address: None,
    };
    let mut header = bincode::serialize(&state).unwrap();
    let mut complement = vec![
        0;
        std::cmp::max(
            0,
            UpgradeableLoaderState::size_of_programdata_metadata().saturating_sub(header.len())
        )
    ];
    let mut buffer = load_program(name);
    header.append(&mut complement);
    header.append(&mut buffer);
    account_data.set_data(header);
    mock_bank
        .account_shared_data
        .write()
        .unwrap()
        .insert(program_data_account, account_data);

    program_account
}
