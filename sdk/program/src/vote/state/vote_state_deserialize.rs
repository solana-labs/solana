use {
    crate::{
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::{BlockTimestamp, LandedVote, Lockout, VoteState, MAX_ITEMS},
    },
    bincode::serialized_size,
    std::io::{Cursor, Read},
};

pub(super) fn deserialize_vote_state_into(
    input: &[u8],
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    has_latency: bool,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = read_pubkey(cursor)?;
    vote_state.authorized_withdrawer = read_pubkey(cursor)?;
    vote_state.commission = read_u8(cursor)?;
    read_votes_into(cursor, vote_state, has_latency)?;
    vote_state.root_slot = read_maybe_u64(cursor)?;
    read_authorized_voters_into(cursor, vote_state)?;

    // `serialized_size()` must be used over `mem::size_of()` because of alignment
    let position_after_circbuf = cursor
        .position()
        .checked_add(
            serialized_size(&vote_state.prior_voters)
                .map_err(|_| InstructionError::InvalidAccountData)?,
        )
        .ok_or(InstructionError::InvalidAccountData)?;

    let is_empty_idx = position_after_circbuf
        .checked_sub(1)
        .ok_or(InstructionError::InvalidAccountData)?;

    match input[is_empty_idx as usize] {
        0 => read_prior_voters_into(cursor, vote_state)?,
        1 => cursor.set_position(position_after_circbuf),
        _ => return Err(InstructionError::InvalidAccountData),
    }

    read_epoch_credits_into(cursor, vote_state)?;
    read_last_timestamp_into(cursor, vote_state)?;

    Ok(())
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, InstructionError> {
    let mut buf = [0; 1];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(buf[0])
}

pub(super) fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, InstructionError> {
    let mut buf = [0; 4];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u32::from_le_bytes(buf))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u64::from_le_bytes(buf))
}

fn read_maybe_u64(cursor: &mut Cursor<&[u8]>) -> Result<Option<u64>, InstructionError> {
    let variant = read_u8(cursor)?;
    match variant {
        0 => Ok(None),
        1 => read_u64(cursor).map(|u| Some(u)),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn read_i64(cursor: &mut Cursor<&[u8]>) -> Result<i64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(i64::from_le_bytes(buf))
}

fn read_pubkey(cursor: &mut Cursor<&[u8]>) -> Result<Pubkey, InstructionError> {
    let mut buf = [0; 32];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(Pubkey::from(buf))
}

fn read_votes_into(
    cursor: &mut Cursor<&[u8]>,
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

fn read_authorized_voters_into(
    cursor: &mut Cursor<&[u8]>,
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

fn read_prior_voters_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    for _ in 0..MAX_ITEMS {
        let prior_voter = read_pubkey(cursor)?;
        let from_epoch = read_u64(cursor)?;
        let until_epoch = read_u64(cursor)?;
        let item = (prior_voter, from_epoch, until_epoch);

        // XXX someone from the core team should confirm this holds
        match item {
            (_, 0, 0) => (),
            _ => vote_state.prior_voters.append(item),
        }
    }

    let idx = read_u64(cursor)?;
    if vote_state.prior_voters.idx != idx as usize {
        return Err(InstructionError::InvalidAccountData);
    }

    let is_empty = read_u8(cursor)?;
    if vote_state.prior_voters.is_empty != (is_empty != 0) {
        return Err(InstructionError::InvalidAccountData);
    }

    Ok(())
}

fn read_epoch_credits_into(
    cursor: &mut Cursor<&[u8]>,
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

fn read_last_timestamp_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let slot = read_u64(cursor)?;
    let timestamp = read_i64(cursor)?;

    vote_state.last_timestamp = BlockTimestamp { slot, timestamp };

    Ok(())
}
