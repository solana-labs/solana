//! budget state
use bincode::{self, deserialize, serialize_into, serialized_size};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_budget_api::budget_expr::BudgetExpr;
use std::io;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BudgetError {
    InsufficientFunds,
    ContractAlreadyExists,
    ContractNotPending,
    SourceIsPendingContract,
    UninitializedContract,
    NegativeTokens,
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
    pub fn is_pending(&self) -> bool {
        self.pending_budget.is_some()
    }

    pub fn serialize(&self, output: &mut [u8]) -> Result<(), BudgetError> {
        let len = serialized_size(self).unwrap() as u64;
        if output.len() < len as usize {
            warn!(
                "{} bytes required to serialize, only have {} bytes",
                len,
                output.len()
            );
            return Err(BudgetError::UserdataTooSmall);
        }
        {
            let writer = io::BufWriter::new(&mut output[..8]);
            serialize_into(writer, &len).unwrap();
        }

        {
            let writer = io::BufWriter::new(&mut output[8..8 + len as usize]);
            serialize_into(writer, self).unwrap();
        }
        Ok(())
    }

    pub fn deserialize(input: &[u8]) -> bincode::Result<Self> {
        if input.len() < 8 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        let len: u64 = deserialize(&input[..8]).unwrap();
        if len < 2 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        if input.len() < 8 + len as usize {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        deserialize(&input[8..8 + len as usize])
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bincode::serialize;
    use solana_budget_api::id;
    use solana_sdk::account::Account;

    #[test]
    fn test_serializer() {
        let mut a = Account::new(0, 512, id());
        let b = BudgetState::default();
        b.serialize(&mut a.userdata).unwrap();
        let buf = serialize(&b).unwrap();
        assert_eq!(a.userdata[8..8 + buf.len()], buf[0..]);
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
