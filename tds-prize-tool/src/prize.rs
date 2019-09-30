use solana_sdk::pubkey::Pubkey;

#[derive(Debug)]
pub enum Category {
    // Availability,
    // ConfirmationLatency,
    RewardsEarned,
}

pub type Winner = (Pubkey, String);

#[derive(Debug)]
pub struct Winners {
    pub category: Category,
    pub top_winners: Vec<Winner>,
    pub bucket_winners: Vec<(String, Vec<Winner>)>,
}
