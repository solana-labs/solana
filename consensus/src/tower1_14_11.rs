use {
    crate::consensus::SwitchForkDecision,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    solana_vote_program::vote_state::{
        vote_state_1_14_11::VoteState1_14_11, BlockTimestamp, VoteTransaction,
    },
};

#[frozen_abi(digest = "F83xHQA1wxoFDy25MTKXXmFXTc9Jbp6SXRXEPcehtKbQ")]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct Tower1_14_11 {
    pub(crate) node_pubkey: Pubkey,
    pub(crate) threshold_depth: usize,
    pub(crate) threshold_size: f64,
    pub(crate) vote_state: VoteState1_14_11,
    pub(crate) last_vote: VoteTransaction,
    #[serde(skip)]
    // The blockhash used in the last vote transaction, may or may not equal the
    // blockhash of the voted block itself, depending if the vote slot was refreshed.
    // For instance, a vote for slot 5, may be refreshed/resubmitted for inclusion in
    //  block 10, in  which case `last_vote_tx_blockhash` equals the blockhash of 10, not 5.
    pub(crate) last_vote_tx_blockhash: Option<Hash>,
    pub(crate) last_timestamp: BlockTimestamp,
    #[serde(skip)]
    // Restored last voted slot which cannot be found in SlotHistory at replayed root
    // (This is a special field for slashing-free validator restart with edge cases).
    // This could be emptied after some time; but left intact indefinitely for easier
    // implementation
    // Further, stray slot can be stale or not. `Stale` here means whether given
    // bank_forks (=~ ledger) lacks the slot or not.
    pub(crate) stray_restored_slot: Option<Slot>,
    #[serde(skip)]
    pub(crate) last_switch_threshold_check: Option<(Slot, SwitchForkDecision)>,
}
