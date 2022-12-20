use std::fmt;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, AbiExample, AbiEnumVisitor, Clone, Copy)]
pub enum RewardType {
    Fee,
    Rent,
    Staking,
    Voting,
    ApplicationFee,
}

impl fmt::Display for RewardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RewardType::Fee => "fee",
                RewardType::Rent => "rent",
                RewardType::Staking => "staking",
                RewardType::Voting => "voting",
                RewardType::ApplicationFee => "application_fee",
            }
        )
    }
}
