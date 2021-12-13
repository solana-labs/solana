use solana_ledger::{
    blockstore::Blockstore,
    shred::{Nonce, SIZE_OF_NONCE},
};
use solana_perf::packet::limited_deserialize;
use solana_sdk::{clock::Slot, packet::Packet};
use std::{io, net::SocketAddr};

pub fn repair_response_packet(
    blockstore: &Blockstore,
    slot: Slot,
    shred_index: u64,
    dest: &SocketAddr,
    nonce: Nonce,
) -> Option<Packet> {
    let shred = blockstore
        .get_data_shred(slot, shred_index)
        .expect("Blockstore could not get data shred");
    shred
        .map(|shred| repair_response_packet_from_shred(shred, dest, nonce))
        .unwrap_or(None)
}

pub fn repair_response_packet_from_shred(
    shred: Vec<u8>,
    dest: &SocketAddr,
    nonce: Nonce,
) -> Option<Packet> {
    let mut packet = Packet::default();
    packet.meta.size = shred.len() + SIZE_OF_NONCE;
    if packet.meta.size > packet.data.len() {
        return None;
    }
    packet.meta.set_addr(dest);
    packet.data[..shred.len()].copy_from_slice(&shred);
    let mut wr = io::Cursor::new(&mut packet.data[shred.len()..]);
    bincode::serialize_into(&mut wr, &nonce).expect("Buffer not large enough to fit nonce");
    Some(packet)
}

pub fn nonce(buf: &[u8]) -> Option<Nonce> {
    if buf.len() < SIZE_OF_NONCE {
        None
    } else {
        limited_deserialize(&buf[buf.len() - SIZE_OF_NONCE..]).ok()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_ledger::{
        shred::{Shred, Shredder},
        sigverify_shreds::verify_shred_cpu,
    };
    use solana_sdk::signature::{Keypair, Signer};
    use std::{
        collections::HashMap,
        net::{IpAddr, Ipv4Addr},
    };

    fn run_test_sigverify_shred_cpu_repair(slot: Slot) {
        solana_logger::setup();
        let mut shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            Some(&[1, 2, 3, 4]),
            true,
            true,
            0,
            0,
            0xc0de,
        );
        assert_eq!(shred.slot(), slot);
        let keypair = Keypair::new();
        Shredder::sign_shred(&keypair, &mut shred);
        trace!("signature {}", shred.common_header.signature);
        let nonce = 9;
        let mut packet = repair_response_packet_from_shred(
            shred.payload,
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            nonce,
        )
        .unwrap();
        packet.meta.repair = true;

        let leader_slots = [(slot, keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, Some(1));

        let wrong_keypair = Keypair::new();
        let leader_slots = [(slot, wrong_keypair.pubkey().to_bytes())]
            .iter()
            .cloned()
            .collect();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, Some(0));

        let leader_slots = HashMap::new();
        let rv = verify_shred_cpu(&packet, &leader_slots);
        assert_eq!(rv, None);
    }

    #[test]
    fn test_sigverify_shred_cpu_repair() {
        run_test_sigverify_shred_cpu_repair(0xdead_c0de);
    }
}
