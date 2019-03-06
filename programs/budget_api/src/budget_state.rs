//! budget state
use crate::budget_expr::BudgetExpr;
use bincode::{self, deserialize, serialize_into};
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BudgetError {
    InsufficientFunds,
    ContractAlreadyExists,
    ContractNotPending,
    SourceIsPendingContract,
    UninitializedContract,
    DestinationMissing,
    FailedWitness,
    UserdataTooSmall,
    UserdataDeserializeFailure,
    UnsignedKey,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BudgetState {
    pub initialized: bool,
    pub pending_budget: Option<BudgetExpr>,
}

impl BudgetState {
    pub fn new(budget_expr: BudgetExpr) -> Self {
        Self {
            initialized: true,
            pending_budget: Some(budget_expr),
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending_budget.is_some()
    }

    pub fn serialize(&self, output: &mut [u8]) -> Result<(), BudgetError> {
        serialize_into(output, self).map_err(|err| match *err {
            _ => BudgetError::UserdataTooSmall,
        })
    }

    pub fn deserialize(input: &[u8]) -> bincode::Result<Self> {
        deserialize(input)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;

    #[test]
    fn test_serializer() {
        let mut a = Account::new(0, 512, id());
        let b = BudgetState::default();
        b.serialize(&mut a.userdata).unwrap();
        let c = BudgetState::deserialize(&a.userdata).unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn test_serializer_userdata_too_small() {
        let mut a = Account::new(0, 1, id());
        let b = BudgetState::default();
        assert_eq!(
            b.serialize(&mut a.userdata),
            Err(BudgetError::UserdataTooSmall)
        );
    }
}
