//! The `packet` module defines data structures and methods to pull data from the network.
use {
    crate::{
        recvmmsg::{recv_mmsg, NUM_RCVMMSGS},
        socket::SocketAddrSpace,
    },
    std::{
        io::Result,
        net::UdpSocket,
        time::{Duration, Instant},
    },
};
pub use {
    solana_perf::packet::{
        to_packet_batches, PacketBatch, PacketBatchRecycler, NUM_PACKETS, PACKETS_PER_BATCH,
    },
    solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE},
};

pub fn recv_from(batch: &mut PacketBatch, socket: &UdpSocket, max_wait: Duration) -> Result<usize> {
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
    loop {
        batch.resize(
            std::cmp::min(i + NUM_RCVMMSGS, PACKETS_PER_BATCH),
            Packet::default(),
        );
        match recv_mmsg(socket, &mut batch[i..]) {
            Err(_) if i > 0 => {
                if start.elapsed() > max_wait {
                    break;
                }
            }
            Err(e) => {
                trace!("recv_from err {:?}", e);
                return Err(e);
            }
            Ok(npkts) => {
                if i == 0 {
                    socket.set_nonblocking(true)?;
                }
                trace!("got {} packets", npkts);
                i += npkts;
                // Try to batch into big enough buffers
                // will cause less re-shuffling later on.
                if start.elapsed() > max_wait || i >= PACKETS_PER_BATCH {
                    break;
                }
            }
        }
    }
    batch.truncate(i);
    Ok(i)
}

pub fn send_to(
    batch: &PacketBatch,
    socket: &UdpSocket,
    socket_addr_space: &SocketAddrSpace,
) -> Result<()> {
    for p in batch.iter() {
        let addr = p.meta().socket_addr();
        if socket_addr_space.check(&addr) {
            if let Some(data) = p.data(..) {
                socket.send_to(data, addr)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            io,
            io::Write,
            net::{SocketAddr, UdpSocket},
        },
    };

    #[test]
    fn test_packets_set_addr() {
        // test that the address is actually being updated
        let send_addr: SocketAddr = "127.0.0.1:123".parse().unwrap();
        let packets = vec![Packet::default()];
        let mut packet_batch = PacketBatch::new(packets);
        packet_batch.set_addr(&send_addr);
        assert_eq!(packet_batch[0].meta().socket_addr(), send_addr);
    }

    #[test]
    pub fn packet_send_recv() {
        solana_logger::setup();
        let recv_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = recv_socket.local_addr().unwrap();
        let send_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = send_socket.local_addr().unwrap();

        let packet_batch_size = 10;
        let mut batch = PacketBatch::with_capacity(packet_batch_size);
        batch.resize(packet_batch_size, Packet::default());

        for m in batch.iter_mut() {
            m.meta_mut().set_socket_addr(&addr);
            m.meta_mut().size = PACKET_DATA_SIZE;
        }
        send_to(&batch, &send_socket, &SocketAddrSpace::Unspecified).unwrap();

        batch
            .iter_mut()
            .for_each(|pkt| *pkt.meta_mut() = Meta::default());
        let recvd = recv_from(
            &mut batch,
            &recv_socket,
            Duration::from_millis(1), // max_wait
        )
        .unwrap();
        assert_eq!(recvd, batch.len());

        for m in batch.iter() {
            assert_eq!(m.meta().size, PACKET_DATA_SIZE);
            assert_eq!(m.meta().socket_addr(), saddr);
        }
    }

    #[test]
    pub fn debug_trait() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", PacketBatch::default()).unwrap();
    }

    #[test]
    fn test_packet_partial_eq() {
        let mut p1 = Packet::default();
        let mut p2 = Packet::default();

        p1.meta_mut().size = 1;
        p1.buffer_mut()[0] = 0;

        p2.meta_mut().size = 1;
        p2.buffer_mut()[0] = 0;

        assert!(p1 == p2);

        p2.buffer_mut()[0] = 4;
        assert!(p1 != p2);
    }

    #[test]
    fn test_packet_resize() {
        solana_logger::setup();
        let recv_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = recv_socket.local_addr().unwrap();
        let send_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let mut batch = PacketBatch::with_capacity(PACKETS_PER_BATCH);
        batch.resize(PACKETS_PER_BATCH, Packet::default());

        // Should only get PACKETS_PER_BATCH packets per iteration even
        // if a lot more were sent, and regardless of packet size
        for _ in 0..2 * PACKETS_PER_BATCH {
            let batch_size = 1;
            let mut batch = PacketBatch::with_capacity(batch_size);
            batch.resize(batch_size, Packet::default());
            for p in batch.iter_mut() {
                p.meta_mut().set_socket_addr(&addr);
                p.meta_mut().size = 1;
            }
            send_to(&batch, &send_socket, &SocketAddrSpace::Unspecified).unwrap();
        }
        let recvd = recv_from(
            &mut batch,
            &recv_socket,
            Duration::from_millis(100), // max_wait
        )
        .unwrap();
        // Check we only got PACKETS_PER_BATCH packets
        assert_eq!(recvd, PACKETS_PER_BATCH);
        assert_eq!(batch.capacity(), PACKETS_PER_BATCH);
    }
}
