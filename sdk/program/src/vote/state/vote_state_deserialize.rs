use {
    crate::{
        instruction::InstructionError,
        pubkey::Pubkey,
        serialize_utils::cursor::*,
        vote::state::{BlockTimestamp, LandedVote, Lockout, VoteState, MAX_ITEMS},
    },
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

    let idx = read_u64(cursor)? as usize;
    vote_state.prior_voters.idx = idx;

    let is_empty_byte = read_u8(cursor)?;
    let is_empty = match is_empty_byte {
        0 => false,
        1 => true,
        _ => return Err(InstructionError::InvalidAccountData),
    };
    vote_state.prior_voters.is_empty = is_empty;

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
