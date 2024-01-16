use {
    crate::{
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::{BlockTimestamp, LandedVote, Lockout, VoteState},
    },
    bincode::serialized_size,
    std::io::{Cursor, Read},
};

// XXX TODO HANA next i want to...
// * move to a pub(super) file
// * use byteorder
// * deser circbuf
// * handle the other variants (implicitly convert to current)
// * split into deserialize_into
// * look into arbitrary and rustfuzz
pub(super) fn deserialize_vote_state(input: &[u8]) -> Result<VoteState, InstructionError> {
    let mut vote_state = VoteState::default();
    let mut cursor = Cursor::new(input);

    let variant = deser_u32(&mut cursor)?;
    if variant != 2 {
        // not supported
        return Err(InstructionError::InvalidAccountData);
    }

    vote_state.node_pubkey = deser_pubkey(&mut cursor)?;
    vote_state.authorized_withdrawer = deser_pubkey(&mut cursor)?;
    vote_state.commission = deser_u8(&mut cursor)?;

    let vote_count = deser_u64(&mut cursor)?;
    for _ in 0..vote_count {
        let latency = deser_u8(&mut cursor)?;
        let slot = deser_u64(&mut cursor)?;
        let confirmation_count = deser_u32(&mut cursor)?;
        let lockout = Lockout::new_with_confirmation_count(slot, confirmation_count);

        vote_state.votes.push_back(LandedVote { latency, lockout });
    }

    vote_state.root_slot = deser_maybe_u64(&mut cursor)?;

    let authorized_voter_count = deser_u64(&mut cursor)?;
    for _ in 0..authorized_voter_count {
        let epoch = deser_u64(&mut cursor)?;
        let authorized_voter = deser_pubkey(&mut cursor)?;

        vote_state.authorized_voters.insert(epoch, authorized_voter);
    }

    let position_after_circbuf = cursor.position()
        + serialized_size(&vote_state.prior_voters)
            .map_err(|_| InstructionError::InvalidAccountData)?;

    match input[position_after_circbuf as usize - 1] {
        0 => unimplemented!(),
        1 => cursor.set_position(position_after_circbuf),
        _ => return Err(InstructionError::InvalidAccountData),
    }

    let epoch_credit_count = deser_u64(&mut cursor)?;
    for _ in 0..epoch_credit_count {
        let epoch = deser_u64(&mut cursor)?;
        let credits = deser_u64(&mut cursor)?;
        let prev_credits = deser_u64(&mut cursor)?;

        vote_state
            .epoch_credits
            .push((epoch, credits, prev_credits));
    }

    {
        let slot = deser_u64(&mut cursor)?;
        let timestamp = deser_i64(&mut cursor)?;

        vote_state.last_timestamp = BlockTimestamp { slot, timestamp };
    }

    Ok(vote_state)
}

fn deser_u8(input: &mut Cursor<&[u8]>) -> Result<u8, InstructionError> {
    let mut buf = [0; 1];
    input
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(buf[0])
}

fn deser_u32(input: &mut Cursor<&[u8]>) -> Result<u32, InstructionError> {
    let mut buf = [0; 4];
    input
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u32::from_le_bytes(buf))
}

fn deser_u64(input: &mut Cursor<&[u8]>) -> Result<u64, InstructionError> {
    let mut buf = [0; 8];
    input
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u64::from_le_bytes(buf))
}

fn deser_maybe_u64(input: &mut Cursor<&[u8]>) -> Result<Option<u64>, InstructionError> {
    let variant = deser_u8(input)?;
    match variant {
        0 => Ok(None),
        1 => deser_u64(input).map(|u| Some(u)),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deser_i64(input: &mut Cursor<&[u8]>) -> Result<i64, InstructionError> {
    let mut buf = [0; 8];
    input
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(i64::from_le_bytes(buf))
}

fn deser_pubkey(input: &mut Cursor<&[u8]>) -> Result<Pubkey, InstructionError> {
    let mut buf = [0; 32];
    input
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(Pubkey::from(buf))
}
