use {
    crate::{
        clock::{Epoch, Slot, UnixTimestamp},
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::{
            vote_state_0_23_5::CircBuf as LegacyCircBuf, vote_state_versions::VoteStateVersions,
            BlockTimestamp, CircBuf as CurrentCircBuf, LandedVote, Lockout, VoteState,
            MAX_ITEMS as CURRENT_MAX_ITEMS,
        },
    },
    bincode::serialized_size,
    num_derive::FromPrimitive,
    num_traits::FromPrimitive,
    std::io::{Cursor, Read},
};

// XXX TODO FIXME i cant import this from vote_state_0_23_5 because its a sibling module
// either i need to move this back into super (ugly) or expose the variable to the crate (ugly)
// or just ignore it because its 32 in both places but ugh
// XXX UPDATE ok i need to move this into super because i now import like every sibling
const LEGACY_MAX_ITEMS: usize = 32;

// this is an internal-use-only enum for parser branching, for clarity over ad hoc booleans
// the From instance is only to ensure both enums advance in lockstep, we dont use it
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

    let position_after_circbuf = cursor.position()
        + serialized_size(&vote_state.prior_voters)
            .map_err(|_| InstructionError::InvalidAccountData)?;

    match input[position_after_circbuf as usize - 1] {
        0 => deser_prior_voters_into(cursor, vote_state, version)?,
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
    let version = InputVersion::V0_23_5;

    // XXX oops this is the right deser order but im not filling out the same struct!
    // XXX also i notice convert to current just throws away 0.23 prior voters...
    vote_state.node_pubkey = deser_pubkey(cursor)?;

    let authorized_voter = deser_pubkey(cursor)?;
    let authorized_voter_epoch = deser_u64(cursor)?;
    vote_state
        .authorized_voters
        .insert(authorized_voter_epoch, authorized_voter);

    deser_prior_voters_into(cursor, vote_state, version)?;
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

// pre-0.23: four-tuple item, no bool
// post-0.23: three-tuple item, is_empty bool
fn deser_prior_voters_into(
    cursor: &mut Cursor<&[u8]>,
    vote_state: &mut VoteState,
    input_version: InputVersion,
) -> Result<(), InstructionError> {
    let max_items = if input_version == InputVersion::V0_23_5 {
        LEGACY_MAX_ITEMS
    } else {
        CURRENT_MAX_ITEMS
    };

    for _ in 0..max_items {
        let prior_voter = deser_pubkey(cursor)?;
        let from_epoch = deser_u64(cursor)?;
        let until_epoch = deser_u64(cursor)?;

        if input_version == InputVersion::V0_23_5 {
            let _slot = deser_u64(cursor)?;
        }

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
    if input_version > InputVersion::V0_23_5 {
        let _is_empty = deser_u8(cursor)?;
    }

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
