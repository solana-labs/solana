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
    trace!("Matching txn: {match_portion:016b} {:016b}", *TXN_MASK);
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
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signature},
            system_transaction,
        },
    };

    #[test]
    fn test_get_signature_from_packet() {
        // Default invalid txn packet
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
        let mut packet = Packet::from_data(None, tx).unwrap();

        let sig = get_signature_from_packet(&packet);
        assert!(sig.is_ok());

        // Invalid signature length
        packet.buffer_mut()[0] = 0x0;
        let sig = get_signature_from_packet(&packet);
        assert_eq!(sig, Err(PacketError::InvalidSignatureLen));
    }

    #[test]
    fn test_should_track_transaction() {
        let mut sig = [0x0; SIGNATURE_BYTES];
        let track = should_track_transaction(&sig);
        assert!(!track);

        // Intentionally matching the randomly generated mask
        // The lower four bits are ignored as only 12 highest bits from
        // signature's 61 and 62 u8 are used for matching.
        // We generate a random one
        let mut rng = rand::thread_rng();
        let random_number: u8 = rng.gen_range(0..=15);
        sig[61] = ((*TXN_MASK & 0xf_u16) << 4) as u8 | random_number;
        sig[62] = (*TXN_MASK >> 4) as u8;

        let track = should_track_transaction(&sig);
        assert!(track);
    }

    #[test]
    fn test_signature_if_should_track_packet() {
        // Default invalid txn packet
        let packet = Packet::default();
        let sig = signature_if_should_track_packet(&packet);
        assert_eq!(sig, Err(PacketError::InvalidShortVec));

        // Use a valid transaction which is not matched
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let packet = Packet::from_data(None, tx).unwrap();
        let sig = signature_if_should_track_packet(&packet);
        assert_eq!(Ok(None), sig);

        // Now simulate a txn matching the signature mask
        let mut tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            1,
            Hash::new_unique(),
        );
        let mut sig = [0x0; SIGNATURE_BYTES];
        sig[61] = ((*TXN_MASK & 0xf_u16) << 4) as u8;
        sig[62] = (*TXN_MASK >> 4) as u8;

        let sig = Signature::from(sig);
        tx.signatures[0] = sig;
        let mut packet = Packet::from_data(None, tx).unwrap();
        let sig2 = signature_if_should_track_packet(&packet);

        match sig2 {
            Ok(sig) => {
                assert!(sig.is_some());
            }
            Err(_) => panic!("Expected to get a matching signature!"),
        }

        // Invalid signature length
        packet.buffer_mut()[0] = 0x0;
        let sig = signature_if_should_track_packet(&packet);
        assert_eq!(sig, Err(PacketError::InvalidSignatureLen));
    }
}
