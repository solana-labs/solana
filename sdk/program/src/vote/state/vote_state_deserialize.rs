use {
    crate::{
        instruction::InstructionError,
        pubkey::Pubkey,
        serialize_utils::cursor::*,
        vote::state::{BlockTimestamp, LandedVote, Lockout, VoteState, MAX_ITEMS},
    },
    bincode::serialized_size,
    std::io::Cursor,
};

pub(super) fn deserialize_vote_state_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    has_latency: bool,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = read_pubkey(cursor)?;
    vote_state.authorized_withdrawer = read_pubkey(cursor)?;
    vote_state.commission = read_u8(cursor)?;
    read_votes_into(cursor, vote_state, has_latency)?;
    vote_state.root_slot = read_option_u64(cursor)?;
    read_authorized_voters_into(cursor, vote_state)?;
    read_prior_voters_into(cursor, vote_state)?;
    read_epoch_credits_into(cursor, vote_state)?;
    read_last_timestamp_into(cursor, vote_state)?;

    Ok(())
}

fn read_votes_into<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
    vote_state: &mut VoteState,
    has_latency: bool,
) -> Result<(), InstructionError> {
    let vote_count = read_u64(cursor)?;

    for _ in 0..vote_count {
        let latency = if has_latency { read_u8(cursor)? } else { 0 };

        let slot = read_u64(cursor)?;
        let confirmation_count = read_u32(cursor)?;
        let lockout = Lockout::new_with_confirmation_count(slot, confirmation_count);

        vote_state.votes.push_back(LandedVote { latency, lockout });
    }

    Ok(())
}

fn read_authorized_voters_into<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let authorized_voter_count = read_u64(cursor)?;

    for _ in 0..authorized_voter_count {
        let epoch = read_u64(cursor)?;
        let authorized_voter = read_pubkey(cursor)?;

        vote_state.authorized_voters.insert(epoch, authorized_voter);
    }

    Ok(())
}

fn read_prior_voters_into<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    // record our position at the start of the struct
    let prior_voters_position = cursor.position();

    // `serialized_size()` must be used over `mem::size_of()` because of alignment
    let is_empty_position = serialized_size(&vote_state.prior_voters)
        .ok()
        .and_then(|v| v.checked_add(prior_voters_position))
        .and_then(|v| v.checked_sub(1))
        .ok_or(InstructionError::InvalidAccountData)?;

    // move to the end, to check if we need to parse the data
    cursor.set_position(is_empty_position);

    // if empty, we already read past the end of this struct and need to do no further work
    // otherwise we go back to the start and proceed to decode the data
    let is_empty = read_bool(cursor)?;
    if !is_empty {
        cursor.set_position(prior_voters_position);

        let mut encountered_null_voter = false;
        for i in 0..MAX_ITEMS {
            let prior_voter = read_pubkey(cursor)?;
            let from_epoch = read_u64(cursor)?;
            let until_epoch = read_u64(cursor)?;
            let item = (prior_voter, from_epoch, until_epoch);

            if item == (Pubkey::default(), 0, 0) {
                encountered_null_voter = true;
            } else if encountered_null_voter {
                // `prior_voters` should never be sparse
                return Err(InstructionError::InvalidAccountData);
            } else {
                vote_state.prior_voters.buf[i] = item;
            }
        }

        vote_state.prior_voters.idx = read_u64(cursor)? as usize;
        vote_state.prior_voters.is_empty = read_bool(cursor)?;
    }

    Ok(())
}

fn read_epoch_credits_into<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let epoch_credit_count = read_u64(cursor)?;

    for _ in 0..epoch_credit_count {
        let epoch = read_u64(cursor)?;
        let credits = read_u64(cursor)?;
        let prev_credits = read_u64(cursor)?;

        vote_state
            .epoch_credits
            .push((epoch, credits, prev_credits));
    }

    Ok(())
}

fn read_last_timestamp_into<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let slot = read_u64(cursor)?;
    let timestamp = read_i64(cursor)?;

    vote_state.last_timestamp = BlockTimestamp { slot, timestamp };

    Ok(())
}
