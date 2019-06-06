//! This account contains the current cluster fees
//!
use crate::account::Account;
use crate::account_utils::State;
use crate::fee_calculator::FeeCalculator;
use crate::pubkey::Pubkey;
use crate::syscall;
use bincode::serialized_size;

/// "Sysca11Fees11111111111111111111111111"
///  fees account pubkey
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 218, 104, 33, 3, 92, 89, 173, 16, 89, 109, 253, 49, 97, 98, 165, 87, 222,
    119, 112, 253, 90, 76, 184, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ID)
}

pub fn check_id(pubkey: &Pubkey) -> bool {
    pubkey.as_ref() == ID
}

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}

impl Fees {
    pub fn from(account: &Account) -> Option<Self> {
        account.state().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.set_state(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&Fees::default()).unwrap() as usize
    }
}

pub fn create_account(lamports: u64) -> Account {
    Account::new(lamports, Fees::size_of(), &syscall::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fees_id() {
        let name = "Sysca11Fees11111111111111111111111111111111";
        // To get the bytes above:
        // dbg!((name, bs58::decode(name).into_vec().unwrap()));
        assert_eq!(name, id().to_string());
        assert!(check_id(&id()));
    }

    #[test]
    fn test_fees_create_account() {
        let lamports = 42;
        let account = create_account(lamports);
        let fees = Fees::from(&account).unwrap();
        assert_eq!(
            fees.fee_calculator.lamports_per_signature,
            FeeCalculator::default().lamports_per_signature
        );
    }
}
