//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When perf-libs are available signature verification is offloaded
//! to the GPU.
//!
use {
    crate::{
        cuda_runtime::PinnedVec,
        packet::{Packet, PacketBatch, PacketFlags, PACKET_DATA_SIZE},
        perf_libs,
        recycler::Recycler,
    },
    rayon::{prelude::*, ThreadPool},
    solana_metrics::inc_new_counter_debug,
    solana_rayon_threadlimit::get_thread_count,
    solana_sdk::{
        hash::Hash,
        message::{MESSAGE_HEADER_LENGTH, MESSAGE_VERSION_PREFIX},
        pubkey::Pubkey,
        short_vec::decode_shortu16_len,
        signature::Signature,
    },
    std::{convert::TryFrom, mem::size_of},
};

// Representing key tKeYE4wtowRb8yRroZShTipE18YVnqwXjsSAoNsFU6g
const TRACER_KEY_BYTES: [u8; 32] = [
    13, 37, 180, 170, 252, 137, 36, 194, 183, 143, 161, 193, 201, 207, 211, 23, 189, 93, 33, 110,
    155, 90, 30, 39, 116, 115, 238, 38, 126, 21, 232, 133,
];
const TRACER_KEY: Pubkey = Pubkey::new_from_array(TRACER_KEY_BYTES);
const TRACER_KEY_OFFSET_IN_TRANSACTION: usize = 69;
// Empirically derived to constrain max verify latency to ~8ms at lower packet counts
pub const VERIFY_PACKET_CHUNK_SIZE: usize = 128;

lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_thread_count())
        .thread_name(|i| format!("solSigVerify{i:02}"))
        .build()
        .unwrap();
}

pub type TxOffset = PinnedVec<u32>;

type TxOffsets = (TxOffset, TxOffset, TxOffset, TxOffset, Vec<Vec<u32>>);

#[derive(Debug, PartialEq, Eq)]
struct PacketOffsets {
    pub sig_len: u32,
    pub sig_start: u32,
    pub msg_start: u32,
    pub pubkey_start: u32,
    pub pubkey_len: u32,
}

impl PacketOffsets {
    pub fn new(
        sig_len: u32,
        sig_start: u32,
        msg_start: u32,
        pubkey_start: u32,
        pubkey_len: u32,
    ) -> Self {
        Self {
            sig_len,
            sig_start,
            msg_start,
            pubkey_start,
            pubkey_len,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PacketError {
    InvalidLen,
    InvalidPubkeyLen,
    InvalidShortVec,
    InvalidSignatureLen,
    MismatchSignatureLen,
    PayerNotWritable,
    InvalidProgramIdIndex,
    InvalidProgramLen,
    UnsupportedVersion,
}

impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for PacketError {
    fn from(_e: std::boxed::Box<bincode::ErrorKind>) -> PacketError {
        PacketError::InvalidShortVec
    }
}

impl std::convert::From<std::num::TryFromIntError> for PacketError {
    fn from(_e: std::num::TryFromIntError) -> Self {
        Self::InvalidLen
    }
}

pub fn init() {
    if let Some(api) = perf_libs::api() {
        unsafe {
            (api.ed25519_set_verbose)(true);
            assert!((api.ed25519_init)(), "ed25519_init() failed");
            (api.ed25519_set_verbose)(false);
        }
    }
}

/// Returns true if the signatrue on the packet verifies.
/// Caller must do packet.set_discard(true) if this returns false.
#[must_use]
fn verify_packet(packet: &mut Packet, reject_non_vote: bool) -> bool {
    // If this packet was already marked as discard, drop it
    if packet.meta().discard() {
        return false;
    }

    let packet_offsets = get_packet_offsets(packet, 0, reject_non_vote);
    let mut sig_start = packet_offsets.sig_start as usize;
    let mut pubkey_start = packet_offsets.pubkey_start as usize;
    let msg_start = packet_offsets.msg_start as usize;

    if packet_offsets.sig_len == 0 {
        return false;
    }

    if packet.meta().size <= msg_start {
        return false;
    }

    for _ in 0..packet_offsets.sig_len {
        let pubkey_end = pubkey_start.saturating_add(size_of::<Pubkey>());
        let Some(sig_end) = sig_start.checked_add(size_of::<Signature>()) else {
            return false;
        };
        let Some(Ok(signature)) = packet.data(sig_start..sig_end).map(Signature::try_from) else {
            return false;
        };
        let Some(pubkey) = packet.data(pubkey_start..pubkey_end) else {
            return false;
        };
        let Some(message) = packet.data(msg_start..) else {
            return false;
        };
        if !signature.verify(pubkey, message) {
            return false;
        }
        pubkey_start = pubkey_end;
        sig_start = sig_end;
    }
    true
}

pub fn count_packets_in_batches(batches: &[PacketBatch]) -> usize {
    batches.iter().map(|batch| batch.len()).sum()
}

pub fn count_valid_packets(
    batches: &[PacketBatch],
    mut process_valid_packet: impl FnMut(&Packet),
) -> usize {
    batches
        .iter()
        .map(|batch| {
            batch
                .iter()
                .filter(|p| {
                    let should_keep = !p.meta().discard();
                    if should_keep {
                        process_valid_packet(p);
                    }
                    should_keep
                })
                .count()
        })
        .sum()
}

pub fn count_discarded_packets(batches: &[PacketBatch]) -> usize {
    batches
        .iter()
        .map(|batch| batch.iter().filter(|p| p.meta().discard()).count())
        .sum()
}

// internal function to be unit-tested; should be used only by get_packet_offsets
fn do_get_packet_offsets(
    packet: &Packet,
    current_offset: usize,
) -> Result<PacketOffsets, PacketError> {
    // should have at least 1 signature and sig lengths
    let _ = 1usize
        .checked_add(size_of::<Signature>())
        .filter(|v| *v <= packet.meta().size)
        .ok_or(PacketError::InvalidLen)?;

    // read the length of Transaction.signatures (serialized with short_vec)
    let (sig_len_untrusted, sig_size) = packet
        .data(..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(PacketError::InvalidShortVec)?;
    // Using msg_start_offset which is based on sig_len_untrusted introduces uncertainty.
    // Ultimately, the actual sigverify will determine the uncertainty.
    let msg_start_offset = sig_len_untrusted
        .checked_mul(size_of::<Signature>())
        .and_then(|v| v.checked_add(sig_size))
        .ok_or(PacketError::InvalidLen)?;

    // Determine the start of the message header by checking the message prefix bit.
    let msg_header_offset = {
        // Packet should have data for prefix bit
        if msg_start_offset >= packet.meta().size {
            return Err(PacketError::InvalidSignatureLen);
        }

        // next byte indicates if the transaction is versioned. If the top bit
        // is set, the remaining bits encode a version number. If the top bit is
        // not set, this byte is the first byte of the message header.
        let message_prefix = *packet
            .data(msg_start_offset)
            .ok_or(PacketError::InvalidSignatureLen)?;
        if message_prefix & MESSAGE_VERSION_PREFIX != 0 {
            let version = message_prefix & !MESSAGE_VERSION_PREFIX;
            match version {
                0 => {
                    // header begins immediately after prefix byte
                    msg_start_offset
                        .checked_add(1)
                        .ok_or(PacketError::InvalidLen)?
                }

                // currently only v0 is supported
                _ => return Err(PacketError::UnsupportedVersion),
            }
        } else {
            msg_start_offset
        }
    };

    let msg_header_offset_plus_one = msg_header_offset
        .checked_add(1)
        .ok_or(PacketError::InvalidLen)?;

    // Packet should have data at least for MessageHeader and 1 byte for Message.account_keys.len
    let _ = msg_header_offset_plus_one
        .checked_add(MESSAGE_HEADER_LENGTH)
        .filter(|v| *v <= packet.meta().size)
        .ok_or(PacketError::InvalidSignatureLen)?;

    // read MessageHeader.num_required_signatures (serialized with u8)
    let sig_len_maybe_trusted = *packet
        .data(msg_header_offset)
        .ok_or(PacketError::InvalidSignatureLen)?;
    let message_account_keys_len_offset = msg_header_offset
        .checked_add(MESSAGE_HEADER_LENGTH)
        .ok_or(PacketError::InvalidSignatureLen)?;

    // This reads and compares the MessageHeader num_required_signatures and
    // num_readonly_signed_accounts bytes. If num_required_signatures is not larger than
    // num_readonly_signed_accounts, the first account is not debitable, and cannot be charged
    // required transaction fees.
    let readonly_signer_offset = msg_header_offset_plus_one;
    if sig_len_maybe_trusted
        <= *packet
            .data(readonly_signer_offset)
            .ok_or(PacketError::InvalidSignatureLen)?
    {
        return Err(PacketError::PayerNotWritable);
    }

    if usize::from(sig_len_maybe_trusted) != sig_len_untrusted {
        return Err(PacketError::MismatchSignatureLen);
    }

    // read the length of Message.account_keys (serialized with short_vec)
    let (pubkey_len, pubkey_len_size) = packet
        .data(message_account_keys_len_offset..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(PacketError::InvalidShortVec)?;
    let pubkey_start = message_account_keys_len_offset
        .checked_add(pubkey_len_size)
        .ok_or(PacketError::InvalidPubkeyLen)?;

    let _ = pubkey_len
        .checked_mul(size_of::<Pubkey>())
        .and_then(|v| v.checked_add(pubkey_start))
        .filter(|v| *v <= packet.meta().size)
        .ok_or(PacketError::InvalidPubkeyLen)?;

    if pubkey_len < sig_len_untrusted {
        return Err(PacketError::InvalidPubkeyLen);
    }

    let sig_start = current_offset
        .checked_add(sig_size)
        .ok_or(PacketError::InvalidLen)?;
    let msg_start = current_offset
        .checked_add(msg_start_offset)
        .ok_or(PacketError::InvalidLen)?;
    let pubkey_start = current_offset
        .checked_add(pubkey_start)
        .ok_or(PacketError::InvalidLen)?;

    Ok(PacketOffsets::new(
        u32::try_from(sig_len_untrusted)?,
        u32::try_from(sig_start)?,
        u32::try_from(msg_start)?,
        u32::try_from(pubkey_start)?,
        u32::try_from(pubkey_len)?,
    ))
}

pub fn check_for_tracer_packet(packet: &mut Packet) -> bool {
    let first_pubkey_start: usize = TRACER_KEY_OFFSET_IN_TRANSACTION;
    let Some(first_pubkey_end) = first_pubkey_start.checked_add(size_of::<Pubkey>()) else {
        return false;
    };
    // Check for tracer pubkey
    match packet.data(first_pubkey_start..first_pubkey_end) {
        Some(pubkey) if pubkey == TRACER_KEY.as_ref() => {
            packet.meta_mut().set_tracer(true);
            true
        }
        _ => false,
    }
}

fn get_packet_offsets(
    packet: &mut Packet,
    current_offset: usize,
    reject_non_vote: bool,
) -> PacketOffsets {
    let unsanitized_packet_offsets = do_get_packet_offsets(packet, current_offset);
    if let Ok(offsets) = unsanitized_packet_offsets {
        check_for_simple_vote_transaction(packet, &offsets, current_offset).ok();
        if !reject_non_vote || packet.meta().is_simple_vote_tx() {
            return offsets;
        }
    }
    // force sigverify to fail by returning zeros
    PacketOffsets::new(0, 0, 0, 0, 0)
}

fn check_for_simple_vote_transaction(
    packet: &mut Packet,
    packet_offsets: &PacketOffsets,
    current_offset: usize,
) -> Result<(), PacketError> {
    // vote could have 1 or 2 sigs; zero sig has already been excluded at
    // do_get_packet_offsets.
    if packet_offsets.sig_len > 2 {
        return Err(PacketError::InvalidSignatureLen);
    }

    // simple vote should only be legacy message
    let msg_start = (packet_offsets.msg_start as usize)
        .checked_sub(current_offset)
        .ok_or(PacketError::InvalidLen)?;
    let message_prefix = *packet.data(msg_start).ok_or(PacketError::InvalidLen)?;
    if message_prefix & MESSAGE_VERSION_PREFIX != 0 {
        return Ok(());
    }

    let pubkey_start = (packet_offsets.pubkey_start as usize)
        .checked_sub(current_offset)
        .ok_or(PacketError::InvalidLen)?;

    let instructions_len_offset = (packet_offsets.pubkey_len as usize)
        .checked_mul(size_of::<Pubkey>())
        .and_then(|v| v.checked_add(pubkey_start))
        .and_then(|v| v.checked_add(size_of::<Hash>()))
        .ok_or(PacketError::InvalidLen)?;

    // Packet should have at least 1 more byte for instructions.len
    let _ = instructions_len_offset
        .checked_add(1usize)
        .filter(|v| *v <= packet.meta().size)
        .ok_or(PacketError::InvalidLen)?;

    let (instruction_len, instruction_len_size) = packet
        .data(instructions_len_offset..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(PacketError::InvalidLen)?;
    // skip if has more than 1 instruction
    if instruction_len != 1 {
        return Err(PacketError::InvalidProgramLen);
    }

    let instruction_start = instructions_len_offset
        .checked_add(instruction_len_size)
        .ok_or(PacketError::InvalidLen)?;

    // Packet should have at least 1 more byte for one instructions_program_id
    let _ = instruction_start
        .checked_add(1usize)
        .filter(|v| *v <= packet.meta().size)
        .ok_or(PacketError::InvalidLen)?;

    let instruction_program_id_index: usize = usize::from(
        *packet
            .data(instruction_start)
            .ok_or(PacketError::InvalidLen)?,
    );

    if instruction_program_id_index >= packet_offsets.pubkey_len as usize {
        return Err(PacketError::InvalidProgramIdIndex);
    }

    let instruction_program_id_start = instruction_program_id_index
        .checked_mul(size_of::<Pubkey>())
        .and_then(|v| v.checked_add(pubkey_start))
        .ok_or(PacketError::InvalidLen)?;
    let instruction_program_id_end = instruction_program_id_start
        .checked_add(size_of::<Pubkey>())
        .ok_or(PacketError::InvalidLen)?;

    if packet
        .data(instruction_program_id_start..instruction_program_id_end)
        .ok_or(PacketError::InvalidLen)?
        == solana_sdk::vote::program::id().as_ref()
    {
        packet.meta_mut().flags |= PacketFlags::SIMPLE_VOTE_TX;
    }
    Ok(())
}

pub fn generate_offsets(
    batches: &mut [PacketBatch],
    recycler: &Recycler<TxOffset>,
    reject_non_vote: bool,
) -> TxOffsets {
    debug!("allocating..");
    let mut signature_offsets: PinnedVec<_> = recycler.allocate("sig_offsets");
    signature_offsets.set_pinnable();
    let mut pubkey_offsets: PinnedVec<_> = recycler.allocate("pubkey_offsets");
    pubkey_offsets.set_pinnable();
    let mut msg_start_offsets: PinnedVec<_> = recycler.allocate("msg_start_offsets");
    msg_start_offsets.set_pinnable();
    let mut msg_sizes: PinnedVec<_> = recycler.allocate("msg_size_offsets");
    msg_sizes.set_pinnable();
    let mut current_offset: usize = 0;
    let offsets = batches
        .iter_mut()
        .map(|batch| {
            batch
                .iter_mut()
                .map(|packet| {
                    let packet_offsets =
                        get_packet_offsets(packet, current_offset, reject_non_vote);

                    trace!("pubkey_offset: {}", packet_offsets.pubkey_start);

                    let mut pubkey_offset = packet_offsets.pubkey_start;
                    let mut sig_offset = packet_offsets.sig_start;
                    let msg_size = current_offset.saturating_add(packet.meta().size) as u32;
                    for _ in 0..packet_offsets.sig_len {
                        signature_offsets.push(sig_offset);
                        sig_offset = sig_offset.saturating_add(size_of::<Signature>() as u32);

                        pubkey_offsets.push(pubkey_offset);
                        pubkey_offset = pubkey_offset.saturating_add(size_of::<Pubkey>() as u32);

                        msg_start_offsets.push(packet_offsets.msg_start);

                        let msg_size = msg_size.saturating_sub(packet_offsets.msg_start);
                        msg_sizes.push(msg_size);
                    }
                    current_offset = current_offset.saturating_add(size_of::<Packet>());
                    packet_offsets.sig_len
                })
                .collect()
        })
        .collect();
    (
        signature_offsets,
        pubkey_offsets,
        msg_start_offsets,
        msg_sizes,
        offsets,
    )
}

//inplace shrink a batch of packets
pub fn shrink_batches(batches: &mut Vec<PacketBatch>) {
    let mut valid_batch_ix = 0;
    let mut valid_packet_ix = 0;
    let mut last_valid_batch = 0;
    for batch_ix in 0..batches.len() {
        for packet_ix in 0..batches[batch_ix].len() {
            if batches[batch_ix][packet_ix].meta().discard() {
                continue;
            }
            last_valid_batch = batch_ix.saturating_add(1);
            let mut found_spot = false;
            while valid_batch_ix < batch_ix && !found_spot {
                while valid_packet_ix < batches[valid_batch_ix].len() {
                    if batches[valid_batch_ix][valid_packet_ix].meta().discard() {
                        batches[valid_batch_ix][valid_packet_ix] =
                            batches[batch_ix][packet_ix].clone();
                        batches[batch_ix][packet_ix].meta_mut().set_discard(true);
                        last_valid_batch = valid_batch_ix.saturating_add(1);
                        found_spot = true;
                        break;
                    }
                    valid_packet_ix = valid_packet_ix.saturating_add(1);
                }
                if valid_packet_ix >= batches[valid_batch_ix].len() {
                    valid_packet_ix = 0;
                    valid_batch_ix = valid_batch_ix.saturating_add(1);
                }
            }
        }
    }
    batches.truncate(last_valid_batch);
}

pub fn ed25519_verify_cpu(batches: &mut [PacketBatch], reject_non_vote: bool, packet_count: usize) {
    debug!("CPU ECDSA for {}", packet_count);
    PAR_THREAD_POOL.install(|| {
        batches
            .par_iter_mut()
            .flatten()
            .collect::<Vec<&mut Packet>>()
            .par_chunks_mut(VERIFY_PACKET_CHUNK_SIZE)
            .for_each(|packets| {
                for packet in packets.iter_mut() {
                    if !packet.meta().discard() && !verify_packet(packet, reject_non_vote) {
                        packet.meta_mut().set_discard(true);
                    }
                }
            });
    });
    inc_new_counter_debug!("ed25519_verify_cpu", packet_count);
}

pub fn ed25519_verify_disabled(batches: &mut [PacketBatch]) {
    let packet_count = count_packets_in_batches(batches);
    debug!("disabled ECDSA for {}", packet_count);
    batches.into_par_iter().for_each(|batch| {
        batch
            .par_iter_mut()
            .for_each(|p| p.meta_mut().set_discard(false))
    });
    inc_new_counter_debug!("ed25519_verify_disabled", packet_count);
}

pub fn copy_return_values<I, T>(sig_lens: I, out: &PinnedVec<u8>, rvs: &mut [Vec<u8>])
where
    I: IntoIterator<Item = T>,
    T: IntoIterator<Item = u32>,
{
    debug_assert!(rvs.iter().flatten().all(|&rv| rv == 0u8));
    let mut offset = 0usize;
    let rvs = rvs.iter_mut().flatten();
    for (k, rv) in sig_lens.into_iter().flatten().zip(rvs) {
        let out = out[offset..].iter().take(k as usize).all(|&x| x == 1u8);
        *rv = u8::from(k != 0u32 && out);
        offset = offset.saturating_add(k as usize);
    }
}

// return true for success, i.e ge unpacks and !ge.is_small_order()
pub fn check_packed_ge_small_order(ge: &[u8; 32]) -> bool {
    if let Some(api) = perf_libs::api() {
        unsafe {
            // Returns 1 == fail, 0 == success
            let res = (api.ed25519_check_packed_ge_small_order)(ge.as_ptr());

            return res == 0;
        }
    }
    false
}

pub fn get_checked_scalar(scalar: &[u8; 32]) -> Result<[u8; 32], PacketError> {
    let mut out = [0u8; 32];
    if let Some(api) = perf_libs::api() {
        unsafe {
            let res = (api.ed25519_get_checked_scalar)(out.as_mut_ptr(), scalar.as_ptr());
            if res == 0 {
                return Ok(out);
            } else {
                return Err(PacketError::InvalidLen);
            }
        }
    }
    Ok(out)
}

pub fn mark_disabled(batches: &mut [PacketBatch], r: &[Vec<u8>]) {
    for (batch, v) in batches.iter_mut().zip(r) {
        for (pkt, f) in batch.iter_mut().zip(v) {
            if !pkt.meta().discard() {
                pkt.meta_mut().set_discard(*f == 0);
            }
        }
    }
}

pub fn ed25519_verify(
    batches: &mut [PacketBatch],
    recycler: &Recycler<TxOffset>,
    recycler_out: &Recycler<PinnedVec<u8>>,
    reject_non_vote: bool,
    valid_packet_count: usize,
) {
    let Some(api) = perf_libs::api() else {
        return ed25519_verify_cpu(batches, reject_non_vote, valid_packet_count);
    };
    let total_packet_count = count_packets_in_batches(batches);
    // micro-benchmarks show GPU time for smallest batch around 15-20ms
    // and CPU speed for 64-128 sigverifies around 10-20ms. 64 is a nice
    // power-of-two number around that accounting for the fact that the CPU
    // may be busy doing other things while being a real validator
    // TODO: dynamically adjust this crossover
    let maybe_valid_percentage = 100usize
        .wrapping_mul(valid_packet_count)
        .checked_div(total_packet_count);
    let Some(valid_percentage) = maybe_valid_percentage else {
        return;
    };
    if valid_percentage < 90 || valid_packet_count < 64 {
        ed25519_verify_cpu(batches, reject_non_vote, valid_packet_count);
        return;
    }

    let (signature_offsets, pubkey_offsets, msg_start_offsets, msg_sizes, sig_lens) =
        generate_offsets(batches, recycler, reject_non_vote);

    debug!("CUDA ECDSA for {}", valid_packet_count);
    debug!("allocating out..");
    let mut out = recycler_out.allocate("out_buffer");
    out.set_pinnable();
    let mut elems = Vec::new();
    let mut rvs = Vec::new();

    let mut num_packets: usize = 0;
    for batch in batches.iter() {
        elems.push(perf_libs::Elems {
            elems: batch.as_ptr().cast::<u8>(),
            num: batch.len() as u32,
        });
        let v = vec![0u8; batch.len()];
        rvs.push(v);
        num_packets = num_packets.saturating_add(batch.len());
    }
    out.resize(signature_offsets.len(), 0);
    trace!("Starting verify num packets: {}", num_packets);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    trace!("len offset: {}", PACKET_DATA_SIZE as u32);
    const USE_NON_DEFAULT_STREAM: u8 = 1;
    unsafe {
        let res = (api.ed25519_verify_many)(
            elems.as_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            num_packets as u32,
            signature_offsets.len() as u32,
            msg_sizes.as_ptr(),
            pubkey_offsets.as_ptr(),
            signature_offsets.as_ptr(),
            msg_start_offsets.as_ptr(),
            out.as_mut_ptr(),
            USE_NON_DEFAULT_STREAM,
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    copy_return_values(sig_lens, &out, &mut rvs);
    mark_disabled(batches, &rvs);
    inc_new_counter_debug!("ed25519_verify_gpu", valid_packet_count);
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)]
mod tests {
    use {
        super::*,
        crate::{
            packet::{to_packet_batches, Packet, PacketBatch, PACKETS_PER_BATCH},
            sigverify::{self, PacketOffsets},
            test_tx::{new_test_vote_tx, test_multisig_tx, test_tx},
        },
        bincode::{deserialize, serialize},
        curve25519_dalek::{edwards::CompressedEdwardsY, scalar::Scalar},
        rand::{thread_rng, Rng},
        solana_sdk::{
            instruction::CompiledInstruction,
            message::{Message, MessageHeader},
            signature::{Keypair, Signature, Signer},
            transaction::Transaction,
        },
        std::{
            iter::repeat_with,
            sync::atomic::{AtomicU64, Ordering},
        },
    };

    const SIG_OFFSET: usize = 1;

    pub fn memfind<A: Eq>(a: &[A], b: &[A]) -> Option<usize> {
        assert!(a.len() >= b.len());
        let end = a.len() - b.len() + 1;
        (0..end).find(|&i| a[i..i + b.len()] == b[..])
    }

    #[test]
    fn test_copy_return_values() {
        let mut rng = rand::thread_rng();
        let sig_lens: Vec<Vec<u32>> = {
            let size = rng.gen_range(0..64);
            repeat_with(|| {
                let size = rng.gen_range(0..16);
                repeat_with(|| rng.gen_range(0..5)).take(size).collect()
            })
            .take(size)
            .collect()
        };
        let out: Vec<Vec<Vec<bool>>> = sig_lens
            .iter()
            .map(|sig_lens| {
                sig_lens
                    .iter()
                    .map(|&size| repeat_with(|| rng.gen()).take(size as usize).collect())
                    .collect()
            })
            .collect();
        let expected: Vec<Vec<u8>> = out
            .iter()
            .map(|out| {
                out.iter()
                    .map(|out| u8::from(!out.is_empty() && out.iter().all(|&k| k)))
                    .collect()
            })
            .collect();
        let out =
            PinnedVec::<u8>::from_vec(out.into_iter().flatten().flatten().map(u8::from).collect());
        let mut rvs: Vec<Vec<u8>> = sig_lens
            .iter()
            .map(|sig_lens| vec![0u8; sig_lens.len()])
            .collect();
        copy_return_values(sig_lens, &out, &mut rvs);
        assert_eq!(rvs, expected);
    }

    #[test]
    fn test_mark_disabled() {
        let batch_size = 1;
        let mut batch = PacketBatch::with_capacity(batch_size);
        batch.resize(batch_size, Packet::default());
        let mut batches: Vec<PacketBatch> = vec![batch];
        mark_disabled(&mut batches, &[vec![0]]);
        assert!(batches[0][0].meta().discard());
        batches[0][0].meta_mut().set_discard(false);
        mark_disabled(&mut batches, &[vec![1]]);
        assert!(!batches[0][0].meta().discard());
    }

    #[test]
    fn test_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let packet = serialize(&tx).unwrap();
        assert_matches!(memfind(&packet, &tx_bytes), Some(0));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }

    #[test]
    fn test_system_transaction_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let message_data = tx.message_data();
        let mut packet = Packet::from_data(None, tx.clone()).unwrap();

        let packet_offsets = sigverify::get_packet_offsets(&mut packet, 0, false);

        assert_eq!(
            memfind(&tx_bytes, tx.signatures[0].as_ref()),
            Some(SIG_OFFSET)
        );
        assert_eq!(
            memfind(&tx_bytes, tx.message().account_keys[0].as_ref()),
            Some(packet_offsets.pubkey_start as usize)
        );
        assert_eq!(
            memfind(&tx_bytes, &message_data),
            Some(packet_offsets.msg_start as usize)
        );
        assert_eq!(
            memfind(&tx_bytes, tx.signatures[0].as_ref()),
            Some(packet_offsets.sig_start as usize)
        );
        assert_eq!(packet_offsets.sig_len, 1);
    }

    fn packet_from_num_sigs(required_num_sigs: u8, actual_num_sigs: usize) -> Packet {
        let message = Message {
            header: MessageHeader {
                num_required_signatures: required_num_sigs,
                num_readonly_signed_accounts: 12,
                num_readonly_unsigned_accounts: 11,
            },
            account_keys: vec![],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let mut tx = Transaction::new_unsigned(message);
        tx.signatures = vec![Signature::default(); actual_num_sigs];
        Packet::from_data(None, tx).unwrap()
    }

    #[test]
    fn test_untrustworthy_sigs() {
        let required_num_sigs = 14;
        let actual_num_sigs = 5;

        let packet = packet_from_num_sigs(required_num_sigs, actual_num_sigs);

        let unsanitized_packet_offsets = sigverify::do_get_packet_offsets(&packet, 0);

        assert_eq!(
            unsanitized_packet_offsets,
            Err(PacketError::MismatchSignatureLen)
        );
    }

    #[test]
    fn test_small_packet() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        packet.buffer_mut()[0] = 0xff;
        packet.buffer_mut()[1] = 0xff;
        packet.meta_mut().size = 2;

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidLen));
    }

    #[test]
    fn test_pubkey_too_small() {
        solana_logger::setup();
        let mut tx = test_tx();
        let sig = tx.signatures[0];
        const NUM_SIG: usize = 18;
        tx.signatures = vec![sig; NUM_SIG];
        tx.message.account_keys = vec![];
        tx.message.header.num_required_signatures = NUM_SIG as u8;
        let mut packet = Packet::from_data(None, tx).unwrap();

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidPubkeyLen));

        assert!(!verify_packet(&mut packet, false));

        packet.meta_mut().set_discard(false);
        let mut batches = generate_packet_batches(&packet, 1, 1);
        ed25519_verify(&mut batches);
        assert!(batches[0][0].meta().discard());
    }

    #[test]
    fn test_pubkey_len() {
        // See that the verify cannot walk off the end of the packet
        // trying to index into the account_keys to access pubkey.
        solana_logger::setup();

        const NUM_SIG: usize = 17;
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let mut message = Message::new(&[], Some(&pubkey1));
        message.account_keys.push(pubkey1);
        message.account_keys.push(pubkey1);
        message.header.num_required_signatures = NUM_SIG as u8;
        message.recent_blockhash = Hash::new_from_array(pubkey1.to_bytes());
        let mut tx = Transaction::new_unsigned(message);

        info!("message: {:?}", tx.message_data());
        info!("tx: {:?}", tx);
        let sig = keypair1.try_sign_message(&tx.message_data()).unwrap();
        tx.signatures = vec![sig; NUM_SIG];

        let mut packet = Packet::from_data(None, tx).unwrap();

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidPubkeyLen));

        assert!(!verify_packet(&mut packet, false));

        packet.meta_mut().set_discard(false);
        let mut batches = generate_packet_batches(&packet, 1, 1);
        ed25519_verify(&mut batches);
        assert!(batches[0][0].meta().discard());
    }

    #[test]
    fn test_large_sig_len() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        // Make the signatures len huge
        packet.buffer_mut()[0] = 0x7f;

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidSignatureLen));
    }

    #[test]
    fn test_really_large_sig_len() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        // Make the signatures len huge
        packet.buffer_mut()[0] = 0xff;
        packet.buffer_mut()[1] = 0xff;
        packet.buffer_mut()[2] = 0xff;
        packet.buffer_mut()[3] = 0xff;

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidShortVec));
    }

    #[test]
    fn test_invalid_pubkey_len() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        let res = sigverify::do_get_packet_offsets(&packet, 0);

        // make pubkey len huge
        packet.buffer_mut()[res.unwrap().pubkey_start as usize - 1] = 0x7f;

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::InvalidPubkeyLen));
    }

    #[test]
    fn test_fee_payer_is_debitable() {
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![],
            recent_blockhash: Hash::default(),
            instructions: vec![],
        };
        let mut tx = Transaction::new_unsigned(message);
        tx.signatures = vec![Signature::default()];
        let packet = Packet::from_data(None, tx).unwrap();
        let res = sigverify::do_get_packet_offsets(&packet, 0);

        assert_eq!(res, Err(PacketError::PayerNotWritable));
    }

    #[test]
    fn test_unsupported_version() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        let res = sigverify::do_get_packet_offsets(&packet, 0);

        // set message version to 1
        packet.buffer_mut()[res.unwrap().msg_start as usize] = MESSAGE_VERSION_PREFIX + 1;

        let res = sigverify::do_get_packet_offsets(&packet, 0);
        assert_eq!(res, Err(PacketError::UnsupportedVersion));
    }

    #[test]
    fn test_versioned_message() {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        let mut legacy_offsets = sigverify::do_get_packet_offsets(&packet, 0).unwrap();

        // set message version to 0
        let msg_start = legacy_offsets.msg_start as usize;
        let msg_bytes = packet.data(msg_start..).unwrap().to_vec();
        packet.buffer_mut()[msg_start] = MESSAGE_VERSION_PREFIX;
        packet.meta_mut().size += 1;
        let msg_end = packet.meta().size;
        packet.buffer_mut()[msg_start + 1..msg_end].copy_from_slice(&msg_bytes);

        let offsets = sigverify::do_get_packet_offsets(&packet, 0).unwrap();
        let expected_offsets = {
            legacy_offsets.pubkey_start += 1;
            legacy_offsets
        };

        assert_eq!(expected_offsets, offsets);
    }

    #[test]
    fn test_system_transaction_data_layout() {
        let mut tx0 = test_tx();
        tx0.message.instructions[0].data = vec![1, 2, 3];
        let message0a = tx0.message_data();
        let tx_bytes = serialize(&tx0).unwrap();
        assert!(tx_bytes.len() <= PACKET_DATA_SIZE);
        assert_eq!(
            memfind(&tx_bytes, tx0.signatures[0].as_ref()),
            Some(SIG_OFFSET)
        );
        let tx1 = deserialize(&tx_bytes).unwrap();
        assert_eq!(tx0, tx1);
        assert_eq!(tx1.message().instructions[0].data, vec![1, 2, 3]);

        tx0.message.instructions[0].data = vec![1, 2, 4];
        let message0b = tx0.message_data();
        assert_ne!(message0a, message0b);
    }

    // Just like get_packet_offsets, but not returning redundant information.
    fn get_packet_offsets_from_tx(tx: Transaction, current_offset: u32) -> PacketOffsets {
        let mut packet = Packet::from_data(None, tx).unwrap();
        let packet_offsets =
            sigverify::get_packet_offsets(&mut packet, current_offset as usize, false);
        PacketOffsets::new(
            packet_offsets.sig_len,
            packet_offsets.sig_start - current_offset,
            packet_offsets.msg_start - packet_offsets.sig_start,
            packet_offsets.pubkey_start - packet_offsets.msg_start,
            packet_offsets.pubkey_len,
        )
    }

    #[test]
    fn test_get_packet_offsets() {
        assert_eq!(
            get_packet_offsets_from_tx(test_tx(), 0),
            PacketOffsets::new(1, 1, 64, 4, 2)
        );
        assert_eq!(
            get_packet_offsets_from_tx(test_tx(), 100),
            PacketOffsets::new(1, 1, 64, 4, 2)
        );

        // Ensure we're not indexing packet by the `current_offset` parameter.
        assert_eq!(
            get_packet_offsets_from_tx(test_tx(), 1_000_000),
            PacketOffsets::new(1, 1, 64, 4, 2)
        );

        // Ensure we're returning sig_len, not sig_size.
        assert_eq!(
            get_packet_offsets_from_tx(test_multisig_tx(), 0),
            PacketOffsets::new(2, 1, 128, 4, 4)
        );
    }

    fn generate_packet_batches_random_size(
        packet: &Packet,
        max_packets_per_batch: usize,
        num_batches: usize,
    ) -> Vec<PacketBatch> {
        // generate packet vector
        let batches: Vec<_> = (0..num_batches)
            .map(|_| {
                let num_packets_per_batch = thread_rng().gen_range(1..max_packets_per_batch);
                let mut packet_batch = PacketBatch::with_capacity(num_packets_per_batch);
                for _ in 0..num_packets_per_batch {
                    packet_batch.push(packet.clone());
                }
                assert_eq!(packet_batch.len(), num_packets_per_batch);
                packet_batch
            })
            .collect();
        assert_eq!(batches.len(), num_batches);

        batches
    }

    fn generate_packet_batches(
        packet: &Packet,
        num_packets_per_batch: usize,
        num_batches: usize,
    ) -> Vec<PacketBatch> {
        // generate packet vector
        let batches: Vec<_> = (0..num_batches)
            .map(|_| {
                let mut packet_batch = PacketBatch::with_capacity(num_packets_per_batch);
                for _ in 0..num_packets_per_batch {
                    packet_batch.push(packet.clone());
                }
                assert_eq!(packet_batch.len(), num_packets_per_batch);
                packet_batch
            })
            .collect();
        assert_eq!(batches.len(), num_batches);

        batches
    }

    fn test_verify_n(n: usize, modify_data: bool) {
        let tx = test_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        // jumble some data to test failure
        if modify_data {
            packet.buffer_mut()[20] = packet.data(20).unwrap().wrapping_add(10);
        }

        let mut batches = generate_packet_batches(&packet, n, 2);

        // verify packets
        ed25519_verify(&mut batches);

        // check result
        let should_discard = modify_data;
        assert!(batches
            .iter()
            .flat_map(|batch| batch.iter())
            .all(|p| p.meta().discard() == should_discard));
    }

    fn ed25519_verify(batches: &mut [PacketBatch]) {
        let recycler = Recycler::default();
        let recycler_out = Recycler::default();
        let packet_count = sigverify::count_packets_in_batches(batches);
        sigverify::ed25519_verify(batches, &recycler, &recycler_out, false, packet_count);
    }

    #[test]
    fn test_verify_tampered_sig_len() {
        let mut tx = test_tx();
        // pretend malicious leader dropped a signature...
        tx.signatures.pop();
        let packet = Packet::from_data(None, tx).unwrap();

        let mut batches = generate_packet_batches(&packet, 1, 1);

        // verify packets
        ed25519_verify(&mut batches);
        assert!(batches
            .iter()
            .flat_map(|batch| batch.iter())
            .all(|p| p.meta().discard()));
    }

    #[test]
    fn test_verify_zero() {
        test_verify_n(0, false);
    }

    #[test]
    fn test_verify_one() {
        test_verify_n(1, false);
    }

    #[test]
    fn test_verify_seventy_one() {
        test_verify_n(71, false);
    }

    #[test]
    fn test_verify_medium_pass() {
        test_verify_n(VERIFY_PACKET_CHUNK_SIZE, false);
    }

    #[test]
    fn test_verify_large_pass() {
        test_verify_n(VERIFY_PACKET_CHUNK_SIZE * get_thread_count(), false);
    }

    #[test]
    fn test_verify_medium_fail() {
        test_verify_n(VERIFY_PACKET_CHUNK_SIZE, true);
    }

    #[test]
    fn test_verify_large_fail() {
        test_verify_n(VERIFY_PACKET_CHUNK_SIZE * get_thread_count(), true);
    }

    #[test]
    fn test_verify_multisig() {
        solana_logger::setup();

        let tx = test_multisig_tx();
        let mut packet = Packet::from_data(None, tx).unwrap();

        let n = 4;
        let num_batches = 3;
        let mut batches = generate_packet_batches(&packet, n, num_batches);

        packet.buffer_mut()[40] = packet.data(40).unwrap().wrapping_add(8);

        batches[0].push(packet);

        // verify packets
        ed25519_verify(&mut batches);

        // check result
        let ref_ans = 1u8;
        let mut ref_vec = vec![vec![ref_ans; n]; num_batches];
        ref_vec[0].push(0u8);
        assert!(batches
            .iter()
            .flat_map(|batch| batch.iter())
            .zip(ref_vec.into_iter().flatten())
            .all(|(p, discard)| {
                if discard == 0 {
                    p.meta().discard()
                } else {
                    !p.meta().discard()
                }
            }));
    }

    #[test]
    fn test_verify_fuzz() {
        solana_logger::setup();

        let tx = test_multisig_tx();
        let packet = Packet::from_data(None, tx).unwrap();

        let recycler = Recycler::default();
        let recycler_out = Recycler::default();
        for _ in 0..50 {
            let num_batches = thread_rng().gen_range(2..30);
            let mut batches = generate_packet_batches_random_size(&packet, 128, num_batches);

            let num_modifications = thread_rng().gen_range(0..5);
            for _ in 0..num_modifications {
                let batch = thread_rng().gen_range(0..batches.len());
                let packet = thread_rng().gen_range(0..batches[batch].len());
                let offset = thread_rng().gen_range(0..batches[batch][packet].meta().size);
                let add = thread_rng().gen_range(0..255);
                batches[batch][packet].buffer_mut()[offset] = batches[batch][packet]
                    .data(offset)
                    .unwrap()
                    .wrapping_add(add);
            }

            let batch_to_disable = thread_rng().gen_range(0..batches.len());
            for p in batches[batch_to_disable].iter_mut() {
                p.meta_mut().set_discard(true);
            }

            // verify from GPU verification pipeline (when GPU verification is enabled) are
            // equivalent to the CPU verification pipeline.
            let mut batches_cpu = batches.clone();
            let packet_count = sigverify::count_packets_in_batches(&batches);
            sigverify::ed25519_verify(&mut batches, &recycler, &recycler_out, false, packet_count);
            ed25519_verify_cpu(&mut batches_cpu, false, packet_count);

            // check result
            batches
                .iter()
                .flat_map(|batch| batch.iter())
                .zip(batches_cpu.iter().flat_map(|batch| batch.iter()))
                .for_each(|(p1, p2)| assert_eq!(p1, p2));
        }
    }

    #[test]
    fn test_verify_fail() {
        test_verify_n(5, true);
    }

    #[test]
    fn test_get_checked_scalar() {
        solana_logger::setup();
        if perf_libs::api().is_none() {
            return;
        }

        let passed_g = AtomicU64::new(0);
        let failed_g = AtomicU64::new(0);
        (0..4).into_par_iter().for_each(|_| {
            let mut input = [0u8; 32];
            let mut passed = 0;
            let mut failed = 0;
            for _ in 0..1_000_000 {
                thread_rng().fill(&mut input);
                let ans = get_checked_scalar(&input);
                let ref_ans = Scalar::from_canonical_bytes(input);
                if let Some(ref_ans) = ref_ans {
                    passed += 1;
                    assert_eq!(ans.unwrap(), ref_ans.to_bytes());
                } else {
                    failed += 1;
                    assert!(ans.is_err());
                }
            }
            passed_g.fetch_add(passed, Ordering::Relaxed);
            failed_g.fetch_add(failed, Ordering::Relaxed);
        });
        info!(
            "passed: {} failed: {}",
            passed_g.load(Ordering::Relaxed),
            failed_g.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn test_ge_small_order() {
        solana_logger::setup();
        if perf_libs::api().is_none() {
            return;
        }

        let passed_g = AtomicU64::new(0);
        let failed_g = AtomicU64::new(0);
        (0..4).into_par_iter().for_each(|_| {
            let mut input = [0u8; 32];
            let mut passed = 0;
            let mut failed = 0;
            for _ in 0..1_000_000 {
                thread_rng().fill(&mut input);
                let ans = check_packed_ge_small_order(&input);
                let ref_ge = CompressedEdwardsY::from_slice(&input);
                if let Some(ref_element) = ref_ge.decompress() {
                    if ref_element.is_small_order() {
                        assert!(!ans);
                    } else {
                        assert!(ans);
                    }
                } else {
                    assert!(!ans);
                }
                if ans {
                    passed += 1;
                } else {
                    failed += 1;
                }
            }
            passed_g.fetch_add(passed, Ordering::Relaxed);
            failed_g.fetch_add(failed, Ordering::Relaxed);
        });
        info!(
            "passed: {} failed: {}",
            passed_g.load(Ordering::Relaxed),
            failed_g.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn test_is_simple_vote_transaction() {
        solana_logger::setup();
        let mut rng = rand::thread_rng();

        // tansfer tx is not
        {
            let mut tx = test_tx();
            tx.message.instructions[0].data = vec![1, 2, 3];
            let mut packet = Packet::from_data(None, tx).unwrap();
            let packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            check_for_simple_vote_transaction(&mut packet, &packet_offsets, 0).ok();
            assert!(!packet.meta().is_simple_vote_tx());
        }

        // single legacy vote tx is
        {
            let mut tx = new_test_vote_tx(&mut rng);
            tx.message.instructions[0].data = vec![1, 2, 3];
            let mut packet = Packet::from_data(None, tx).unwrap();
            let packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            check_for_simple_vote_transaction(&mut packet, &packet_offsets, 0).ok();
            assert!(packet.meta().is_simple_vote_tx());
        }

        // single versioned vote tx is not
        {
            let mut tx = new_test_vote_tx(&mut rng);
            tx.message.instructions[0].data = vec![1, 2, 3];
            let mut packet = Packet::from_data(None, tx).unwrap();

            // set messager version to v0
            let mut packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            let msg_start = packet_offsets.msg_start as usize;
            let msg_bytes = packet.data(msg_start..).unwrap().to_vec();
            packet.buffer_mut()[msg_start] = MESSAGE_VERSION_PREFIX;
            packet.meta_mut().size += 1;
            let msg_end = packet.meta().size;
            packet.buffer_mut()[msg_start + 1..msg_end].copy_from_slice(&msg_bytes);

            packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            check_for_simple_vote_transaction(&mut packet, &packet_offsets, 0).ok();
            assert!(!packet.meta().is_simple_vote_tx());
        }

        // multiple mixed tx is not
        {
            let key = Keypair::new();
            let key1 = Pubkey::new_unique();
            let key2 = Pubkey::new_unique();
            let tx = Transaction::new_with_compiled_instructions(
                &[&key],
                &[key1, key2],
                Hash::default(),
                vec![solana_vote_program::id(), Pubkey::new_unique()],
                vec![
                    CompiledInstruction::new(3, &(), vec![0, 1]),
                    CompiledInstruction::new(4, &(), vec![0, 2]),
                ],
            );
            let mut packet = Packet::from_data(None, tx).unwrap();
            let packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            check_for_simple_vote_transaction(&mut packet, &packet_offsets, 0).ok();
            assert!(!packet.meta().is_simple_vote_tx());
        }

        // single legacy vote tx with extra (invalid) signature is not
        {
            let mut tx = new_test_vote_tx(&mut rng);
            tx.signatures.push(Signature::default());
            tx.message.header.num_required_signatures = 3;
            tx.message.instructions[0].data = vec![1, 2, 3];
            let mut packet = Packet::from_data(None, tx).unwrap();
            let packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            assert_eq!(
                Err(PacketError::InvalidSignatureLen),
                check_for_simple_vote_transaction(&mut packet, &packet_offsets, 0)
            );
            assert!(!packet.meta().is_simple_vote_tx());
        }
    }

    #[test]
    fn test_is_simple_vote_transaction_with_offsets() {
        solana_logger::setup();
        let mut rng = rand::thread_rng();

        // batch of legacy messages
        {
            let mut current_offset = 0usize;
            let mut batch = PacketBatch::default();
            batch.push(Packet::from_data(None, test_tx()).unwrap());
            let tx = new_test_vote_tx(&mut rng);
            batch.push(Packet::from_data(None, tx).unwrap());
            batch.iter_mut().enumerate().for_each(|(index, packet)| {
                let packet_offsets = do_get_packet_offsets(packet, current_offset).unwrap();
                check_for_simple_vote_transaction(packet, &packet_offsets, current_offset).ok();
                if index == 1 {
                    assert!(packet.meta().is_simple_vote_tx());
                } else {
                    assert!(!packet.meta().is_simple_vote_tx());
                }

                current_offset = current_offset.saturating_add(size_of::<Packet>());
            });
        }

        // batch of mixed legacy messages and versioned vote tx, which won't be flagged as
        // simple_vote_tx
        {
            let mut current_offset = 0usize;
            let mut batch = PacketBatch::default();
            batch.push(Packet::from_data(None, test_tx()).unwrap());
            // versioned vote tx
            let tx = new_test_vote_tx(&mut rng);
            let mut packet = Packet::from_data(None, tx).unwrap();
            let packet_offsets = do_get_packet_offsets(&packet, 0).unwrap();
            let msg_start = packet_offsets.msg_start as usize;
            let msg_bytes = packet.data(msg_start..).unwrap().to_vec();
            packet.buffer_mut()[msg_start] = MESSAGE_VERSION_PREFIX;
            packet.meta_mut().size += 1;
            let msg_end = packet.meta().size;
            packet.buffer_mut()[msg_start + 1..msg_end].copy_from_slice(&msg_bytes);
            batch.push(packet);

            batch.iter_mut().for_each(|packet| {
                let packet_offsets = do_get_packet_offsets(packet, current_offset).unwrap();
                check_for_simple_vote_transaction(packet, &packet_offsets, current_offset).ok();
                assert!(!packet.meta().is_simple_vote_tx());

                current_offset = current_offset.saturating_add(size_of::<Packet>());
            });
        }
    }

    #[test]
    fn test_shrink_fuzz() {
        for _ in 0..5 {
            let mut batches = to_packet_batches(
                &(0..PACKETS_PER_BATCH * 3)
                    .map(|_| test_tx())
                    .collect::<Vec<_>>(),
                PACKETS_PER_BATCH,
            );
            batches.iter_mut().for_each(|b| {
                b.iter_mut()
                    .for_each(|p| p.meta_mut().set_discard(thread_rng().gen()))
            });
            //find all the non discarded packets
            let mut start = vec![];
            batches.iter_mut().for_each(|b| {
                b.iter_mut()
                    .filter(|p| !p.meta().discard())
                    .for_each(|p| start.push(p.clone()))
            });
            start.sort_by(|a, b| a.data(..).cmp(&b.data(..)));

            let packet_count = count_valid_packets(&batches, |_| ());
            shrink_batches(&mut batches);

            //make sure all the non discarded packets are the same
            let mut end = vec![];
            batches.iter_mut().for_each(|b| {
                b.iter_mut()
                    .filter(|p| !p.meta().discard())
                    .for_each(|p| end.push(p.clone()))
            });
            end.sort_by(|a, b| a.data(..).cmp(&b.data(..)));
            let packet_count2 = count_valid_packets(&batches, |_| ());
            assert_eq!(packet_count, packet_count2);
            assert_eq!(start, end);
        }
    }

    #[test]
    fn test_shrink_empty() {
        const PACKET_COUNT: usize = 1024;
        const BATCH_COUNT: usize = PACKET_COUNT / PACKETS_PER_BATCH;

        // No batches
        // truncate of 1 on len 0 is a noop
        shrink_batches(&mut Vec::new());
        // One empty batch
        {
            let mut batches = vec![PacketBatch::with_capacity(0)];
            shrink_batches(&mut batches);
            assert_eq!(batches.len(), 0);
        }
        // Many empty batches
        {
            let mut batches = (0..BATCH_COUNT)
                .map(|_| PacketBatch::with_capacity(0))
                .collect::<Vec<_>>();
            shrink_batches(&mut batches);
            assert_eq!(batches.len(), 0);
        }
    }

    #[test]
    fn test_shrink_vectors() {
        const PACKET_COUNT: usize = 1024;
        const BATCH_COUNT: usize = PACKET_COUNT / PACKETS_PER_BATCH;

        let set_discards = [
            // contiguous
            // 0
            // No discards
            |_, _| false,
            // All discards
            |_, _| true,
            // single partitions
            // discard last half of packets
            |b, p| ((b * PACKETS_PER_BATCH) + p) >= (PACKET_COUNT / 2),
            // discard first half of packets
            |b, p| ((b * PACKETS_PER_BATCH) + p) < (PACKET_COUNT / 2),
            // discard last half of each batch
            |_, p| p >= (PACKETS_PER_BATCH / 2),
            // 5
            // discard first half of each batch
            |_, p| p < (PACKETS_PER_BATCH / 2),
            // uniform sparse
            // discard even packets
            |b, p| ((b * PACKETS_PER_BATCH) + p) % 2 == 0,
            // discard odd packets
            |b, p| ((b * PACKETS_PER_BATCH) + p) % 2 == 1,
            // discard even batches
            |b, _| b % 2 == 0,
            // discard odd batches
            |b, _| b % 2 == 1,
            // edges
            // 10
            // discard first batch
            |b, _| b == 0,
            // discard last batch
            |b, _| b == BATCH_COUNT - 1,
            // discard first and last batches
            |b, _| b == 0 || b == BATCH_COUNT - 1,
            // discard all but first and last batches
            |b, _| b != 0 && b != BATCH_COUNT - 1,
            // discard first packet
            |b, p| ((b * PACKETS_PER_BATCH) + p) == 0,
            // 15
            // discard all but first packet
            |b, p| ((b * PACKETS_PER_BATCH) + p) != 0,
            // discard last packet
            |b, p| ((b * PACKETS_PER_BATCH) + p) == PACKET_COUNT - 1,
            // discard all but last packet
            |b, p| ((b * PACKETS_PER_BATCH) + p) != PACKET_COUNT - 1,
            // discard first packet of each batch
            |_, p| p == 0,
            // discard all but first packet of each batch
            |_, p| p != 0,
            // 20
            // discard last packet of each batch
            |_, p| p == PACKETS_PER_BATCH - 1,
            // discard all but last packet of each batch
            |_, p| p != PACKETS_PER_BATCH - 1,
            // discard first and last packet of each batch
            |_, p| p == 0 || p == PACKETS_PER_BATCH - 1,
            // discard all but first and last packet of each batch
            |_, p| p != 0 && p != PACKETS_PER_BATCH - 1,
            // discard all after first packet in second to last batch
            |b, p| (b == BATCH_COUNT - 2 && p > 0) || b == BATCH_COUNT - 1,
            // 25
        ];

        let expect_valids = [
            // (expected_batches, expected_valid_packets)
            //
            // contiguous
            // 0
            (BATCH_COUNT, PACKET_COUNT),
            (0, 0),
            // single partitions
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            // 5
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            // uniform sparse
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            (BATCH_COUNT / 2, PACKET_COUNT / 2),
            // edges
            // 10
            (BATCH_COUNT - 1, PACKET_COUNT - PACKETS_PER_BATCH),
            (BATCH_COUNT - 1, PACKET_COUNT - PACKETS_PER_BATCH),
            (BATCH_COUNT - 2, PACKET_COUNT - 2 * PACKETS_PER_BATCH),
            (2, 2 * PACKETS_PER_BATCH),
            (BATCH_COUNT, PACKET_COUNT - 1),
            // 15
            (1, 1),
            (BATCH_COUNT, PACKET_COUNT - 1),
            (1, 1),
            (
                (BATCH_COUNT * (PACKETS_PER_BATCH - 1) + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                (PACKETS_PER_BATCH - 1) * BATCH_COUNT,
            ),
            (
                (BATCH_COUNT + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                BATCH_COUNT,
            ),
            // 20
            (
                (BATCH_COUNT * (PACKETS_PER_BATCH - 1) + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                (PACKETS_PER_BATCH - 1) * BATCH_COUNT,
            ),
            (
                (BATCH_COUNT + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                BATCH_COUNT,
            ),
            (
                (BATCH_COUNT * (PACKETS_PER_BATCH - 2) + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                (PACKETS_PER_BATCH - 2) * BATCH_COUNT,
            ),
            (
                (2 * BATCH_COUNT + PACKETS_PER_BATCH) / PACKETS_PER_BATCH,
                PACKET_COUNT - (PACKETS_PER_BATCH - 2) * BATCH_COUNT,
            ),
            (BATCH_COUNT - 1, PACKET_COUNT - 2 * PACKETS_PER_BATCH + 1),
            // 25
        ];

        let test_cases = set_discards.iter().zip(&expect_valids).enumerate();
        for (i, (set_discard, (expect_batch_count, expect_valid_packets))) in test_cases {
            debug!("test_shrink case: {}", i);
            let mut batches = to_packet_batches(
                &(0..PACKET_COUNT).map(|_| test_tx()).collect::<Vec<_>>(),
                PACKETS_PER_BATCH,
            );
            assert_eq!(batches.len(), BATCH_COUNT);
            assert_eq!(count_valid_packets(&batches, |_| ()), PACKET_COUNT);
            batches.iter_mut().enumerate().for_each(|(i, b)| {
                b.iter_mut()
                    .enumerate()
                    .for_each(|(j, p)| p.meta_mut().set_discard(set_discard(i, j)))
            });
            assert_eq!(count_valid_packets(&batches, |_| ()), *expect_valid_packets);
            debug!("show valid packets for case {}", i);
            batches.iter_mut().enumerate().for_each(|(i, b)| {
                b.iter_mut().enumerate().for_each(|(j, p)| {
                    if !p.meta().discard() {
                        trace!("{} {}", i, j)
                    }
                })
            });
            debug!("done show valid packets for case {}", i);
            shrink_batches(&mut batches);
            let shrunken_batch_count = batches.len();
            debug!("shrunk batch test {} count: {}", i, shrunken_batch_count);
            assert_eq!(shrunken_batch_count, *expect_batch_count);
            assert_eq!(count_valid_packets(&batches, |_| ()), *expect_valid_packets);
        }
    }
}
