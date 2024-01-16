use {
    crate::{
        clock::{Epoch, Slot, UnixTimestamp},
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::{
            vote_state_0_23_5::CircBuf as LegacyCircBuf, BlockTimestamp, CircBuf as CurrentCircBuf,
            LandedVote, Lockout, VoteState, MAX_ITEMS as CURRENT_MAX_ITEMS,
        },
    },
    bincode::serialized_size,
    std::io::{Cursor, Read},
};

// XXX TODO FIXME i cant import this from vote_state_0_23_5 because its a sibling module
// either i need to move this back into super (ugly) or expose the variable to the crate (ugly)
const LEGACY_MAX_ITEMS: usize = 32;

// XXX TODO HANA next i want to...
// X move to a pub(super) file
// X use byteorder
// X deser circbuf
// * handle the other variants (implicitly convert to current)
// X split into deserialize_into
// * look into arbitrary and rustfuzz
pub(super) fn deserialize_vote_state_into(
    input: &[u8],
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let mut cursor = Cursor::new(input);

    let variant = deser_u32(&mut cursor)?;
    match variant {
        2 => deserialize_current_vote_state_into(input, vote_state, &mut cursor),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deserialize_current_vote_state_into(
    input: &[u8],
    vote_state: &mut VoteState,
    cursor: &mut Cursor<&[u8]>,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = deser_pubkey(cursor)?;
    vote_state.authorized_withdrawer = deser_pubkey(cursor)?;
    vote_state.commission = deser_u8(cursor)?;

    let vote_count = deser_u64(cursor)?;
    for _ in 0..vote_count {
        let latency = deser_u8(cursor)?;
        let slot = deser_u64(cursor)?;
        let confirmation_count = deser_u32(cursor)?;
        let lockout = Lockout::new_with_confirmation_count(slot, confirmation_count);

        vote_state.votes.push_back(LandedVote { latency, lockout });
    }

    vote_state.root_slot = deser_maybe_u64(cursor)?;

    let authorized_voter_count = deser_u64(cursor)?;
    for _ in 0..authorized_voter_count {
        let epoch = deser_u64(cursor)?;
        let authorized_voter = deser_pubkey(cursor)?;

        vote_state.authorized_voters.insert(epoch, authorized_voter);
    }

    let position_after_circbuf = cursor.position()
        + serialized_size(&vote_state.prior_voters)
            .map_err(|_| InstructionError::InvalidAccountData)?;

    println!("HANA position before: {}", cursor.position());
    match input[position_after_circbuf as usize - 1] {
        0 => deser_current_circbuf_into(cursor, &mut vote_state.prior_voters)?,
        1 => cursor.set_position(position_after_circbuf),
        _ => return Err(InstructionError::InvalidAccountData),
    }
    println!("HANA position after: {}", cursor.position());

    let epoch_credit_count = deser_u64(cursor)?;
    for _ in 0..epoch_credit_count {
        let epoch = deser_u64(cursor)?;
        let credits = deser_u64(cursor)?;
        let prev_credits = deser_u64(cursor)?;

        vote_state
            .epoch_credits
            .push((epoch, credits, prev_credits));
    }

    {
        let slot = deser_u64(cursor)?;
        let timestamp = deser_i64(cursor)?;

        vote_state.last_timestamp = BlockTimestamp { slot, timestamp };
    }

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

fn deser_current_circbuf_into(
    cursor: &mut Cursor<&[u8]>,
    prior_voters: &mut CurrentCircBuf<(Pubkey, Epoch, Epoch)>,
) -> Result<(), InstructionError> {
    for _ in 0..CURRENT_MAX_ITEMS {
        let prior_voter = deser_pubkey(cursor)?;
        let from_epoch = deser_u64(cursor)?;
        let until_epoch = deser_u64(cursor)?;
        let item = (prior_voter, from_epoch, until_epoch);

        // XXX im ~90% sure this is correct in all cases but others should check
        // we have to skip placeholder values, otherwise the index is thrown off
        // ...then again i just realized this struct is in our module so i guess we can ignore encapsulation lol
        match item {
            (_, 0, 0) => (),
            _ => prior_voters.append(item),
        }
    }

    // XXX can check that these are right to be cute
    let _idx = deser_u64(cursor)?;
    let _is_empty = deser_u8(cursor)?;

    Ok(())
}

fn deser_legacy_circbuf_into(
    cursor: &mut Cursor<&[u8]>,
    prior_voters: &mut LegacyCircBuf<(Pubkey, Epoch, Epoch, Slot)>,
) -> Result<(), InstructionError> {
    for _ in 0..LEGACY_MAX_ITEMS {
        let prior_voter = deser_pubkey(cursor)?;
        let from_epoch = deser_u64(cursor)?;
        let until_epoch = deser_u64(cursor)?;
        let slot = deser_u64(cursor)?;
        let item = (prior_voter, from_epoch, until_epoch, slot);

        // XXX note above
        match item {
            (_, 0, 0, 0) => (),
            _ => prior_voters.append(item),
        }
    }

    // XXX note above
    let _idx = deser_u64(cursor)?;

    Ok(())
}
