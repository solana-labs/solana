use {
    lazy_static::lazy_static,
    log::*,
    rand::Rng,
    solana_perf::sigverify::PacketError,
    solana_sdk::{packet::Packet, short_vec::decode_shortu16_len, signature::SIGNATURE_BYTES},
};

// The mask is 12 bits long (1<<12 = 4096), it means the probability of matching
// the transaction is 1/4096 assuming the portion being matched is random.
lazy_static! {
    static ref TXN_MASK: u16 = rand::thread_rng().gen_range(0..4096);
}

/// Check if a transaction given its signature matches the randomly selected mask.
/// The signaure should be from the reference of Signature
pub fn should_track_transaction(signature: &[u8; SIGNATURE_BYTES]) -> bool {
    // We do not use the highest signature byte as it is not really random
    let match_portion: u16 = u16::from_le_bytes([signature[61], signature[62]]) >> 4;
    trace!("Matching txn: {match_portion:b} {:b}", *TXN_MASK);
    *TXN_MASK == match_portion
}

/// Check if a transaction packet's signature matches the mask.
/// This does a rudimentry verification to make sure the packet at least
/// contains the signature data and it returns the reference to the signature.
pub fn signature_if_should_track_packet(
    packet: &Packet,
) -> Result<Option<&[u8; SIGNATURE_BYTES]>, PacketError> {
    let signature = get_signature_from_packet(packet)?;
    Ok(should_track_transaction(signature).then_some(signature))
}

/// Get the signature of the transaction packet
/// This does a rudimentry verification to make sure the packet at least
/// contains the signature data and it returns the reference to the signature.
pub fn get_signature_from_packet(packet: &Packet) -> Result<&[u8; SIGNATURE_BYTES], PacketError> {
    let (sig_len_untrusted, sig_start) = packet
        .data(..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(PacketError::InvalidShortVec)?;

    println!("Sig length is okay!!");
    if sig_len_untrusted < 1 {
        return Err(PacketError::InvalidSignatureLen);
    }

    let signature = packet
        .data(sig_start..sig_start.saturating_add(SIGNATURE_BYTES))
        .ok_or(PacketError::InvalidSignatureLen)?;
    let signature = signature
        .try_into()
        .map_err(|_| PacketError::InvalidSignatureLen)?;
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{hash::Hash, signature::Keypair, system_transaction},
    };

    #[test]
    fn test_get_signature_from_packet() {
        // default invalid txn packet
        let packet = Packet::default();
        let sig = get_signature_from_packet(&packet);
        assert_eq!(sig, Err(PacketError::InvalidShortVec));

        // Use a valid transaction, it should succeed
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, tx).unwrap();

        let sig = get_signature_from_packet(&packet);
        assert!(sig.is_ok());
    }
}
