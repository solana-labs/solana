//! system smart contract

use bank::Account;
use bincode::deserialize;
use signature::Pubkey;
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SystemContract {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * tokens - number of tokens to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * contract - the contract id of the new account
    CreateAccount {
        tokens: i64,
        space: u64,
        contract_id: Option<Pubkey>,
    },
    /// Assign account to a contract
    /// * Transaction::keys[0] - account to assign
    Assign { contract_id: Pubkey },
    /// Move tokens
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { tokens: i64 },
}

pub const SYSTEM_CONTRACT_ID: [u8; 32] = [0u8; 32];

impl SystemContract {
    pub fn check_id(contract_id: &Pubkey) -> bool {
        contract_id.as_ref() == SYSTEM_CONTRACT_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&SYSTEM_CONTRACT_ID)
    }
    pub fn get_balance(account: &Account) -> i64 {
        account.tokens
    }
    pub fn process_transaction(tx: &Transaction, accounts: &mut [Account]) {
        let syscall: SystemContract = deserialize(&tx.userdata).unwrap();
        match syscall {
            SystemContract::CreateAccount {
                tokens,
                space,
                contract_id,
            } => {
                if !Self::check_id(&accounts[1].contract_id) {
                    return;
                }
                if !Self::check_id(&accounts[0].contract_id) {
                    return;
                }
                if space > 0 && !accounts[1].userdata.is_empty() {
                    return;
                }
                accounts[0].tokens -= tokens;
                accounts[1].tokens += tokens;
                if let Some(id) = contract_id {
                    accounts[1].contract_id = id;
                }
                accounts[1].userdata = vec![0; space as usize];
            }
            SystemContract::Assign { contract_id } => {
                if !Self::check_id(&accounts[0].contract_id) {
                    return;
                }
                accounts[0].contract_id = contract_id;
            }
            SystemContract::Move { tokens } => {
                //bank should be verifying correctness
                accounts[0].tokens -= tokens;
                accounts[1].tokens += tokens;
            }
        }
    }
}
