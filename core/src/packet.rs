//! The `packet` module defines data structures and methods to pull data from the network.
use crate::{
    recvmmsg::{recv_mmsg, NUM_RCVMMSGS},
    result::{Error, Result},
};
pub use solana_perf::packet::{
    limited_deserialize, to_packets, to_packets_chunked, Packets, PacketsRecycler, NUM_PACKETS,
    PACKETS_BATCH_SIZE, PACKETS_PER_BATCH,
};

use solana_metrics::inc_new_counter_debug;
pub use solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE};
use std::{net::UdpSocket, time::Instant};

pub fn recv_from(obj: &mut Packets, socket: &UdpSocket) -> Result<usize> {
    let mut i = 0;
    //DOCUMENTED SIDE-EFFECT
    //Performance out of the IO without poll
    //  * block on the socket until it's readable
    //  * set the socket to non blocking
    //  * read until it fails
    //  * set it back to blocking before returning
    socket.set_nonblocking(false)?;
    trace!("receiving on {}", socket.local_addr().unwrap());
    let start = Instant::now();
    let mut total_size = 0;
    loop {
        obj.packets.resize(i + NUM_RCVMMSGS, Packet::default());
        match recv_mmsg(socket, &mut obj.packets[i..]) {
            Err(_) if i > 0 => {
                if start.elapsed().as_millis() > 1 {
                    break;
                }
            }
            Err(e) => {
                trace!("recv_from err {:?}", e);
                return Err(Error::IO(e));
            }
            Ok((size, npkts)) => {
                if i == 0 {
                    socket.set_nonblocking(true)?;
                }
                trace!("got {} packets", npkts);
                i += npkts;
                total_size += size;
                // Try to batch into big enough buffers
                // will cause less re-shuffling later on.
                if start.elapsed().as_millis() > 1 || total_size >= PACKETS_BATCH_SIZE {
                    break;
                }
            }
        }
    }
    obj.packets.truncate(i);
    inc_new_counter_debug!("packets-recv_count", i);
    Ok(i)
}

pub fn send_to(obj: &Packets, socket: &UdpSocket) -> Result<()> {
    for p in &obj.packets {
        let a = p.meta.addr();
        socket.send_to(&p.data[..p.meta.size], &a)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::io::Write;
    use std::net::{SocketAddr, UdpSocket};

    #[test]
    fn test_packets_set_addr() {
        // test that the address is actually being updated
        let send_addr = socketaddr!([127, 0, 0, 1], 123);
        let packets = vec![Packet::default()];
        let mut msgs = Packets::new(packets);
        msgs.set_addr(&send_addr);
        assert_eq!(SocketAddr::from(msgs.packets[0].meta.addr()), send_addr);
    }

    #[test]
    pub fn packet_send_recv() {
        solana_logger::setup();
        let recv_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = recv_socket.local_addr().unwrap();
        let send_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = send_socket.local_addr().unwrap();
        let mut p = Packets::default();

        p.packets.resize(10, Packet::default());

        for m in p.packets.iter_mut() {
            m.meta.set_addr(&addr);
            m.meta.size = PACKET_DATA_SIZE;
        }
        send_to(&p, &send_socket).unwrap();

        let recvd = recv_from(&mut p, &recv_socket).unwrap();

        assert_eq!(recvd, p.packets.len());

        for m in &p.packets {
            assert_eq!(m.meta.size, PACKET_DATA_SIZE);
            assert_eq!(m.meta.addr(), saddr);
        }
    }

    #[test]
    pub fn debug_trait() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
    }

    #[test]
    fn test_packet_partial_eq() {
        let mut p1 = Packet::default();
        let mut p2 = Packet::default();

        p1.meta.size = 1;
        p1.data[0] = 0;

        p2.meta.size = 1;
        p2.data[0] = 0;

        assert!(p1 == p2);

        p2.data[0] = 4;
        assert!(p1 != p2);
    }
}
