//! Vote program instructions

use {
    crate::{
        clock::Slot,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        system_instruction, sysvar,
        vote::{
            program::id,
            state::{
                serde_compact_vote_state_update, Vote, VoteAuthorize,
                VoteAuthorizeCheckedWithSeedArgs, VoteAuthorizeWithSeedArgs, VoteInit, VoteState,
                VoteStateUpdate,
            },
        },
    },
    serde_derive::{Deserialize, Serialize},
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize a vote account
    ///
    /// # Account references
    ///   0. `[WRITE]` Uninitialized vote account
    ///   1. `[]` Rent sysvar
    ///   2. `[]` Clock sysvar
    ///   3. `[SIGNER]` New validator identity (node_pubkey)
    InitializeAccount(VoteInit),

    /// Authorize a key to send votes or issue a withdrawal
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to be updated with the Pubkey for authorization
    ///   1. `[]` Clock sysvar
    ///   2. `[SIGNER]` Vote or withdraw authority
    Authorize(Pubkey, VoteAuthorize),

    /// A Vote instruction with recent votes
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to vote with
    ///   1. `[]` Slot hashes sysvar
    ///   2. `[]` Clock sysvar
    ///   3. `[SIGNER]` Vote authority
    Vote(Vote),

    /// Withdraw some amount of funds
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to withdraw from
    ///   1. `[WRITE]` Recipient account
    ///   2. `[SIGNER]` Withdraw authority
    Withdraw(u64),

    /// Update the vote account's validator identity (node_pubkey)
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to be updated with the given authority public key
    ///   1. `[SIGNER]` New validator identity (node_pubkey)
    ///   2. `[SIGNER]` Withdraw authority
    UpdateValidatorIdentity,

    /// Update the commission for the vote account
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to be updated
    ///   1. `[SIGNER]` Withdraw authority
    UpdateCommission(u8),

    /// A Vote instruction with recent votes
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to vote with
    ///   1. `[]` Slot hashes sysvar
    ///   2. `[]` Clock sysvar
    ///   3. `[SIGNER]` Vote authority
    VoteSwitch(Vote, Hash),

    /// Authorize a key to send votes or issue a withdrawal
    ///
    /// This instruction behaves like `Authorize` with the additional requirement that the new vote
    /// or withdraw authority must also be a signer.
    ///
    /// # Account references
    ///   0. `[WRITE]` Vote account to be updated with the Pubkey for authorization
    ///   1. `[]` Clock sysvar
    ///   2. `[SIGNER]` Vote or withdraw authority
    ///   3. `[SIGNER]` New vote or withdraw authority
    AuthorizeChecked(VoteAuthorize),

    /// Update the onchain vote state for the signer.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to vote with
    ///   1. `[SIGNER]` Vote authority
    UpdateVoteState(VoteStateUpdate),

    /// Update the onchain vote state for the signer along with a switching proof.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to vote with
    ///   1. `[SIGNER]` Vote authority
    UpdateVoteStateSwitch(VoteStateUpdate, Hash),

    /// Given that the current Voter or Withdrawer authority is a derived key,
    /// this instruction allows someone who can sign for that derived key's
    /// base key to authorize a new Voter or Withdrawer for a vote account.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to be updated
    ///   1. `[]` Clock sysvar
    ///   2. `[SIGNER]` Base key of current Voter or Withdrawer authority's derived key
    AuthorizeWithSeed(VoteAuthorizeWithSeedArgs),

    /// Given that the current Voter or Withdrawer authority is a derived key,
    /// this instruction allows someone who can sign for that derived key's
    /// base key to authorize a new Voter or Withdrawer for a vote account.
    ///
    /// This instruction behaves like `AuthorizeWithSeed` with the additional requirement
    /// that the new vote or withdraw authority must also be a signer.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to be updated
    ///   1. `[]` Clock sysvar
    ///   2. `[SIGNER]` Base key of current Voter or Withdrawer authority's derived key
    ///   3. `[SIGNER]` New vote or withdraw authority
    AuthorizeCheckedWithSeed(VoteAuthorizeCheckedWithSeedArgs),

    /// Update the onchain vote state for the signer.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to vote with
    ///   1. `[SIGNER]` Vote authority
    #[serde(with = "serde_compact_vote_state_update")]
    CompactUpdateVoteState(VoteStateUpdate),

    /// Update the onchain vote state for the signer along with a switching proof.
    ///
    /// # Account references
    ///   0. `[Write]` Vote account to vote with
    ///   1. `[SIGNER]` Vote authority
    CompactUpdateVoteStateSwitch(
        #[serde(with = "serde_compact_vote_state_update")] VoteStateUpdate,
        Hash,
    ),
}

impl VoteInstruction {
    pub fn is_simple_vote(&self) -> bool {
        matches!(
            self,
            Self::Vote(_)
                | Self::VoteSwitch(_, _)
                | Self::UpdateVoteState(_)
                | Self::UpdateVoteStateSwitch(_, _)
                | Self::CompactUpdateVoteState(_)
                | Self::CompactUpdateVoteStateSwitch(_, _),
        )
    }

    pub fn is_single_vote_state_update(&self) -> bool {
        matches!(
            self,
            Self::UpdateVoteState(_)
                | Self::UpdateVoteStateSwitch(_, _)
                | Self::CompactUpdateVoteState(_)
                | Self::CompactUpdateVoteStateSwitch(_, _),
        )
    }

    /// Only to be used on vote instructions (guard with is_simple_vote),  panics otherwise
    pub fn last_voted_slot(&self) -> Option<Slot> {
        assert!(self.is_simple_vote());
        match self {
            Self::Vote(v) | Self::VoteSwitch(v, _) => v.last_voted_slot(),
            Self::UpdateVoteState(vote_state_update)
            | Self::UpdateVoteStateSwitch(vote_state_update, _)
            | Self::CompactUpdateVoteState(vote_state_update)
            | Self::CompactUpdateVoteStateSwitch(vote_state_update, _) => {
                vote_state_update.last_voted_slot()
            }
            _ => panic!("Tried to get slot on non simple vote instruction"),
        }
    }
}

fn initialize_account(vote_pubkey: &Pubkey, vote_init: &VoteInit) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(vote_init.node_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::InitializeAccount(*vote_init),
        account_metas,
    )
}

pub fn create_account(
    from_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    vote_init: &VoteInit,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix =
        system_instruction::create_account(from_pubkey, vote_pubkey, lamports, space, &id());
    let init_ix = initialize_account(vote_pubkey, vote_init);
    vec![create_ix, init_ix]
}

pub fn create_account_with_seed(
    from_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    base: &Pubkey,
    seed: &str,
    vote_init: &VoteInit,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix = system_instruction::create_account_with_seed(
        from_pubkey,
        vote_pubkey,
        base,
        seed,
        lamports,
        space,
        &id(),
    );
    let init_ix = initialize_account(vote_pubkey, vote_init);
    vec![create_ix, init_ix]
}

pub fn authorize(
    vote_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey, // currently authorized
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::Authorize(*new_authorized_pubkey, vote_authorize),
        account_metas,
    )
}

pub fn authorize_checked(
    vote_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey, // currently authorized
    new_authorized_pubkey: &Pubkey,
    vote_authorize: VoteAuthorize,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
        AccountMeta::new_readonly(*new_authorized_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::AuthorizeChecked(vote_authorize),
        account_metas,
    )
}

pub fn authorize_with_seed(
    vote_pubkey: &Pubkey,
    current_authority_base_key: &Pubkey,
    current_authority_derived_key_owner: &Pubkey,
    current_authority_derived_key_seed: &str,
    new_authority: &Pubkey,
    authorization_type: VoteAuthorize,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*current_authority_base_key, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::AuthorizeWithSeed(VoteAuthorizeWithSeedArgs {
            authorization_type,
            current_authority_derived_key_owner: *current_authority_derived_key_owner,
            current_authority_derived_key_seed: current_authority_derived_key_seed.to_string(),
            new_authority: *new_authority,
        }),
        account_metas,
    )
}

pub fn authorize_checked_with_seed(
    vote_pubkey: &Pubkey,
    current_authority_base_key: &Pubkey,
    current_authority_derived_key_owner: &Pubkey,
    current_authority_derived_key_seed: &str,
    new_authority: &Pubkey,
    authorization_type: VoteAuthorize,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*current_authority_base_key, true),
        AccountMeta::new_readonly(*new_authority, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::AuthorizeCheckedWithSeed(VoteAuthorizeCheckedWithSeedArgs {
            authorization_type,
            current_authority_derived_key_owner: *current_authority_derived_key_owner,
            current_authority_derived_key_seed: current_authority_derived_key_seed.to_string(),
        }),
        account_metas,
    )
}

pub fn update_validator_identity(
    vote_pubkey: &Pubkey,
    authorized_withdrawer_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*node_pubkey, true),
        AccountMeta::new_readonly(*authorized_withdrawer_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::UpdateValidatorIdentity,
        account_metas,
    )
}

pub fn update_commission(
    vote_pubkey: &Pubkey,
    authorized_withdrawer_pubkey: &Pubkey,
    commission: u8,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*authorized_withdrawer_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::UpdateCommission(commission),
        account_metas,
    )
}

pub fn vote(vote_pubkey: &Pubkey, authorized_voter_pubkey: &Pubkey, vote: Vote) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::slot_hashes::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(id(), &VoteInstruction::Vote(vote), account_metas)
}

pub fn vote_switch(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    vote: Vote,
    proof_hash: Hash,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::slot_hashes::id(), false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::VoteSwitch(vote, proof_hash),
        account_metas,
    )
}

pub fn update_vote_state(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    vote_state_update: VoteStateUpdate,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::UpdateVoteState(vote_state_update),
        account_metas,
    )
}

pub fn update_vote_state_switch(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    vote_state_update: VoteStateUpdate,
    proof_hash: Hash,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::UpdateVoteStateSwitch(vote_state_update, proof_hash),
        account_metas,
    )
}

pub fn compact_update_vote_state(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    vote_state_update: VoteStateUpdate,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::CompactUpdateVoteState(vote_state_update),
        account_metas,
    )
}

pub fn compact_update_vote_state_switch(
    vote_pubkey: &Pubkey,
    authorized_voter_pubkey: &Pubkey,
    vote_state_update: VoteStateUpdate,
    proof_hash: Hash,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_readonly(*authorized_voter_pubkey, true),
    ];

    Instruction::new_with_bincode(
        id(),
        &VoteInstruction::CompactUpdateVoteStateSwitch(vote_state_update, proof_hash),
        account_metas,
    )
}

pub fn withdraw(
    vote_pubkey: &Pubkey,
    authorized_withdrawer_pubkey: &Pubkey,
    lamports: u64,
    to_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_readonly(*authorized_withdrawer_pubkey, true),
    ];

    Instruction::new_with_bincode(id(), &VoteInstruction::Withdraw(lamports), account_metas)
}
