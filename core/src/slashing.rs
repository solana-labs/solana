use solana_vote_program::vote_state::{
    VoteState,
};
use solana_sdk::{
    clock::{Slot},
};

///! find a slot that would be slashable between the two vote states
///! - nca:  newest common ancestor
///! - a:  vote state A
///! - b:  vote state B
///! @retval: (A's slashable slot in B, B's slashable slot in A)
pub fn find_slashable_slot(
    nca: Slot,
    a: &VoteState,
    b: &VoteState,
    a_ancestors: &HashSet<Slot>,
    b_ancestors: &HashSet<Slot>,
) -> (Option<Slot>, Option<Slot>)
{
    let mut a_in_b = None;
    let mut b_in_a = None;
    //iterate starting from oldest to newest vote
    let mut cur_a = a.votes.iter();
    let mut cur_b = b.votes.iter();
    while cur_.is_some() && cur_b.is_some() {
        if cur_a.slot < nca {
            cur_a.next();
            continue;
        } 
        if cur_b.slot < nca {
            cur_b.next();
            continue;
        }
        if b_ancestors.contains(cur_a.slot) {
            cur_a.next();
            continue;
        }
        if a_ancestors.contains(cur_b.slot) {
            cur_b.next();
            continue;
        }
        //a and b eare not ancestors of each other
        //so they must be expired to be valid
        if cur_a.slot > cur_b.slot && !cur_b.is_expired(cur_a.slot) {
            a_in_b =  Some(cur_a.slot);
            cur_a.next();
            continue;
        }
        if cur_b.slot > cur_a.slot && !cur_a.is_expired(cur_b.slot) {
            b_in_a = Some(cur_b.slot);
            cur_b.next();
            continue;
        }
        //move the lowest lockout first
        if cur_a.slot < cur_b.slot {
            cur_a.next();
        } else {
            cur_b.next();
        }
    }
    (a_in_b, b_in_a)
}
