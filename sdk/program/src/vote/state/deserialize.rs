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

// this is an internal-use-only enum for parser branching, for clarity over ad hoc booleans
// the From instance is only to ensure both enums advance in lockstep, we dont use it
// XXX i originally added this enum expecting to parse more things differently based on version... might remove
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
enum InputVersion {
    V0_23_5 = 0,
    V1_14_11,
    Current,
}
impl From<VoteStateVersions> for InputVersion {
    fn from(version: VoteStateVersions) -> Self {
        match version {
            VoteStateVersions::V0_23_5(_) => InputVersion::V0_23_5,
            VoteStateVersions::V1_14_11(_) => InputVersion::V1_14_11,
            VoteStateVersions::Current(_) => InputVersion::Current,
        }
    }
}

pub(super) fn deserialize_vote_state_into(
    input: &[u8],
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    let mut cursor = Cursor::new(input);

    let variant = deser_u32(&mut cursor)?;
    match InputVersion::from_u32(variant) {
        Some(version) if version == InputVersion::Current || version == InputVersion::V1_14_11 => {
            deserialize_modern_vote_state_into(input, &mut cursor, vote_state, version)
        }
        Some(InputVersion::V0_23_5) => deserialize_vote_state_0_23_5_into(&mut cursor, vote_state),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deserialize_modern_vote_state_into(
    input: &[u8],
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    version: InputVersion,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = deser_pubkey(cursor)?;
    vote_state.authorized_withdrawer = deser_pubkey(cursor)?;
    vote_state.commission = deser_u8(cursor)?;
    deser_votes_into(cursor, vote_state, version)?;
    vote_state.root_slot = deser_maybe_u64(cursor)?;
    deser_authorized_voters_into(cursor, vote_state)?;

    // `serialized_size()` *must* be used here because of alignment
    // XXX check if this costs much compute tho and hardcode if so
    // XXX checked add
    let position_after_circbuf = cursor.position()
        + serialized_size(&vote_state.prior_voters)
            .map_err(|_| InstructionError::InvalidAccountData)?;

    match input[position_after_circbuf as usize - 1] {
        0 => deser_prior_voters_into(cursor, vote_state)?,
        1 => cursor.set_position(position_after_circbuf),
        _ => return Err(InstructionError::InvalidAccountData),
    }

    deser_epoch_credits_into(cursor, vote_state)?;
    deser_last_timestamp_into(cursor, vote_state)?;

    Ok(())
}

fn deserialize_vote_state_0_23_5_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
) -> Result<(), InstructionError> {
    vote_state.node_pubkey = deser_pubkey(cursor)?;

    let authorized_voter = deser_pubkey(cursor)?;
    let authorized_voter_epoch = deser_u64(cursor)?;
    vote_state
        .authorized_voters
        .insert(authorized_voter_epoch, authorized_voter);

    // skip `prior_voters` in keeping with the behavior of `convert_to_current()`
    // `size_of()` is ok here because theres no bool, and we avoid an allocation
    // XXX checked add
    let position_after_circbuf = cursor.position()
        + std::mem::size_of::<LegacyCircBuf<(Pubkey, Epoch, Epoch, Slot)>>() as u64;
    cursor.set_position(position_after_circbuf);

    vote_state.authorized_withdrawer = deser_pubkey(cursor)?;
    vote_state.commission = deser_u8(cursor)?;
    vote_state.root_slot = deser_maybe_u64(cursor)?;
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
    input_version: InputVersion,
) -> Result<(), InstructionError> {
    let vote_count = deser_u64(cursor)?;

    for _ in 0..vote_count {
        let latency = if input_version > InputVersion::V1_14_11 {
            deser_u8(cursor)?
        } else {
            0
        };

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
