use {
    crate::{
        clock::{Epoch, Slot},
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::{
            vote_state_0_23_5::CircBuf as LegacyCircBuf, vote_state_versions::VoteStateVersions,
            BlockTimestamp, LandedVote, Lockout, VoteState, MAX_ITEMS,
        },
    },
    bincode::serialized_size,
    num_derive::FromPrimitive,
    num_traits::FromPrimitive,
    std::io::{Cursor, Read},
};

// XXX TODO look into arbitrary and rustfuzz for testing
// XXX maybe i should to move this files contents back into super because i import several siblings

// XXX this becomes same fn in mod.rs
pub(super) fn deserialize_into(
    input: &[u8],
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let mut cursor = Cursor::new(input);

    let variant = deser_u32(&mut cursor)?;
    match variant {
        // 0_23_5. not supported; these should not exist on mainnet
        0 => Err(InstructionError::InvalidAccountData),
        // 1_14_11. substantially different layout and data
        1 => deserialize_vote_state_into(input, &mut cursor, vote_state, false),
        // Current. the only difference is the addition of a slot-latency to each vote
        2 => deserialize_vote_state_into(input, &mut cursor, vote_state, true),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deserialize_vote_state_into(
    input: &[u8],
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    has_latency: bool,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = deser_pubkey(cursor)?;
    vote_state.authorized_withdrawer = deser_pubkey(cursor)?;
    vote_state.commission = deser_u8(cursor)?;
    deser_votes_into(cursor, vote_state, has_latency)?;
    vote_state.root_slot = deser_maybe_u64(cursor)?;
    deser_authorized_voters_into(cursor, vote_state)?;

    // `serialized_size()` *must* be used here because of alignment
    let position_after_circbuf = cursor
        .position()
        .checked_add(
            serialized_size(&vote_state.prior_voters)
                .map_err(|_| InstructionError::InvalidAccountData)?,
        )
        .ok_or(InstructionError::InvalidAccountData)?;

    match input[position_after_circbuf as usize - 1] {
        0 => deser_prior_voters_into(cursor, vote_state)?,
        1 => cursor.set_position(position_after_circbuf),
        _ => return Err(InstructionError::InvalidAccountData),
    }

    deser_epoch_credits_into(cursor, vote_state)?;
    deser_last_timestamp_into(cursor, vote_state)?;

    Ok(())
}

// XXX the deser number functions could be replaced with byteorder?
// but its not presently imported in sdk/program so i dont want to just add it for no reason

fn deser_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, InstructionError> {
    let mut buf = [0; 1];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(buf[0])
}

fn deser_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, InstructionError> {
    let mut buf = [0; 4];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u32::from_le_bytes(buf))
}

fn deser_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u64::from_le_bytes(buf))
}

fn deser_maybe_u64(cursor: &mut Cursor<&[u8]>) -> Result<Option<u64>, InstructionError> {
    let variant = deser_u8(cursor)?;
    match variant {
        0 => Ok(None),
        1 => deser_u64(cursor).map(|u| Some(u)),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deser_i64(cursor: &mut Cursor<&[u8]>) -> Result<i64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(i64::from_le_bytes(buf))
}

fn deser_pubkey(cursor: &mut Cursor<&[u8]>) -> Result<Pubkey, InstructionError> {
    let mut buf = [0; 32];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(Pubkey::from(buf))
}

// pre-1.14: vec of lockouts
// post-1.14: vec of landed votes (lockout plus latency)
fn deser_votes_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    has_latency: bool,
) -> Result<(), InstructionError> {
    let vote_count = deser_u64(cursor)?;

    for _ in 0..vote_count {
        let latency = if has_latency { deser_u8(cursor)? } else { 0 };

        let slot = deser_u64(cursor)?;
        let confirmation_count = deser_u32(cursor)?;
        let lockout = Lockout::new_with_confirmation_count(slot, confirmation_count);

        vote_state.votes.push_back(LandedVote { latency, lockout });
    }

    Ok(())
}

fn deser_authorized_voters_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let authorized_voter_count = deser_u64(cursor)?;

    for _ in 0..authorized_voter_count {
        let epoch = deser_u64(cursor)?;
        let authorized_voter = deser_pubkey(cursor)?;

        vote_state.authorized_voters.insert(epoch, authorized_voter);
    }

    Ok(())
}

fn deser_prior_voters_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    for _ in 0..MAX_ITEMS {
        let prior_voter = deser_pubkey(cursor)?;
        let from_epoch = deser_u64(cursor)?;
        let until_epoch = deser_u64(cursor)?;
        let item = (prior_voter, from_epoch, until_epoch);

        // XXX im ~90% sure this is correct in all cases but others should check
        // we have to skip placeholder values, otherwise the index is thrown off
        // ...then again i just realized this struct is in our module so i guess we can ignore encapsulation lol
        match item {
            (_, 0, 0) => (),
            _ => vote_state.prior_voters.append(item),
        }
    }

    // XXX can check that these are right to be cute
    let _idx = deser_u64(cursor)?;
    let _is_empty = deser_u8(cursor)?;

    Ok(())
}

fn deser_epoch_credits_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let epoch_credit_count = deser_u64(cursor)?;

    for _ in 0..epoch_credit_count {
        let epoch = deser_u64(cursor)?;
        let credits = deser_u64(cursor)?;
        let prev_credits = deser_u64(cursor)?;

        vote_state
            .epoch_credits
            .push((epoch, credits, prev_credits));
    }

    Ok(())
}

fn deser_last_timestamp_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let slot = deser_u64(cursor)?;
    let timestamp = deser_i64(cursor)?;

    vote_state.last_timestamp = BlockTimestamp { slot, timestamp };

    Ok(())
}
