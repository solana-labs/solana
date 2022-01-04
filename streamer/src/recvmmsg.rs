//! The `recvmmsg` module provides recvmmsg() API implementation

pub use solana_perf::packet::NUM_RCVMMSGS;
use {
    crate::packet::Packet,
    std::{cmp, io, net::UdpSocket},
};
#[cfg(target_os = "linux")]
use {
    itertools::izip,
    libc::{iovec, mmsghdr, sockaddr_storage, socklen_t, AF_INET, AF_INET6, MSG_WAITFORONE},
    nix::sys::socket::InetAddr,
    std::{mem, os::unix::io::AsRawFd},
};

#[cfg(not(target_os = "linux"))]
pub fn recv_mmsg(socket: &UdpSocket, packets: &mut [Packet]) -> io::Result</*num packets:*/ usize> {
    let mut i = 0;
    let count = cmp::min(NUM_RCVMMSGS, packets.len());
    for p in packets.iter_mut().take(count) {
        p.meta.size = 0;
        match socket.recv_from(&mut p.data) {
            Err(_) if i > 0 => {
                break;
            }
            Err(e) => {
                return Err(e);
            }
            Ok((nrecv, from)) => {
                p.meta.size = nrecv;
                p.meta.set_addr(&from);
                if i == 0 {
                    socket.set_nonblocking(true)?;
                }
            }
        }
        i += 1;
    }
    Ok(i)
}

#[cfg(target_os = "linux")]
fn cast_socket_addr(addr: &sockaddr_storage, hdr: &mmsghdr) -> Option<InetAddr> {
    use libc::{sa_family_t, sockaddr_in, sockaddr_in6};
    const SOCKADDR_IN_SIZE: usize = std::mem::size_of::<sockaddr_in>();
    const SOCKADDR_IN6_SIZE: usize = std::mem::size_of::<sockaddr_in6>();
    if addr.ss_family == AF_INET as sa_family_t
        && hdr.msg_hdr.msg_namelen == SOCKADDR_IN_SIZE as socklen_t
    {
        let addr = addr as *const _ as *const sockaddr_in;
        return Some(unsafe { InetAddr::V4(*addr) });
    }
    if addr.ss_family == AF_INET6 as sa_family_t
        && hdr.msg_hdr.msg_namelen == SOCKADDR_IN6_SIZE as socklen_t
    {
        let addr = addr as *const _ as *const sockaddr_in6;
        return Some(unsafe { InetAddr::V6(*addr) });
    }
    error!(
        "recvmmsg unexpected ss_family:{} msg_namelen:{}",
        addr.ss_family, hdr.msg_hdr.msg_namelen
    );
    None
}

#[cfg(target_os = "linux")]
#[allow(clippy::uninit_assumed_init)]
pub fn recv_mmsg(sock: &UdpSocket, packets: &mut [Packet]) -> io::Result</*num packets:*/ usize> {
    const SOCKADDR_STORAGE_SIZE: usize = mem::size_of::<sockaddr_storage>();

    let mut hdrs: [mmsghdr; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let mut iovs: [iovec; NUM_RCVMMSGS] = unsafe { mem::MaybeUninit::uninit().assume_init() };
    let mut addrs: [sockaddr_storage; NUM_RCVMMSGS] = unsafe { mem::zeroed() };

    let sock_fd = sock.as_raw_fd();
    let count = cmp::min(iovs.len(), packets.len());

    for (packet, hdr, iov, addr) in
        izip!(packets.iter_mut(), &mut hdrs, &mut iovs, &mut addrs).take(count)
    {
        *iov = iovec {
            iov_base: packet.data.as_mut_ptr() as *mut libc::c_void,
            iov_len: packet.data.len(),
        };
        hdr.msg_hdr.msg_name = addr as *mut _ as *mut _;
        hdr.msg_hdr.msg_namelen = SOCKADDR_STORAGE_SIZE as socklen_t;
        hdr.msg_hdr.msg_iov = iov;
        hdr.msg_hdr.msg_iovlen = 1;
    }
    let mut ts = libc::timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    let nrecv =
        unsafe { libc::recvmmsg(sock_fd, &mut hdrs[0], count as u32, MSG_WAITFORONE, &mut ts) };
    if nrecv < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut npkts = 0;

    izip!(addrs, hdrs, packets.iter_mut())
        .take(nrecv as usize)
        .filter_map(|(addr, hdr, pkt)| {
            let addr = cast_socket_addr(&addr, &hdr)?.to_std();
            Some((addr, hdr, pkt))
        })
        .for_each(|(addr, hdr, pkt)| {
            pkt.meta.size = hdr.msg_len as usize;
            pkt.meta.set_addr(&addr);
            npkts += 1;
        });
    Ok(npkts)
}

#[cfg(test)]
mod tests {
    use {
        crate::{packet::PACKET_DATA_SIZE, recvmmsg::*},
        std::{
            net::{SocketAddr, UdpSocket},
            time::{Duration, Instant},
        },
    };

    type TestConfig = (UdpSocket, SocketAddr, UdpSocket, SocketAddr);

    fn test_setup_reader_sender(ip_str: &str) -> io::Result<TestConfig> {
        let reader = UdpSocket::bind(ip_str)?;
        let addr = reader.local_addr()?;
        let sender = UdpSocket::bind(ip_str)?;
        let saddr = sender.local_addr()?;
        Ok((reader, addr, sender, saddr))
    }

    const TEST_NUM_MSGS: usize = 32;
    #[test]
    pub fn test_recv_mmsg_one_iter() {
        let test_one_iter = |(reader, addr, sender, saddr): TestConfig| {
            let sent = TEST_NUM_MSGS - 1;
            for _ in 0..sent {
                let data = [0; PACKET_DATA_SIZE];
                sender.send_to(&data[..], &addr).unwrap();
            }

            let mut packets = vec![Packet::default(); TEST_NUM_MSGS];
            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(sent, recv);
            for packet in packets.iter().take(recv) {
                assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
                assert_eq!(packet.meta.addr(), saddr);
            }
        };

        test_one_iter(test_setup_reader_sender("127.0.0.1:0").unwrap());

        match test_setup_reader_sender("::1:0") {
            Ok(config) => test_one_iter(config),
            Err(e) => warn!("Failed to configure IPv6: {:?}", e),
        }
    }

    #[test]
    pub fn test_recv_mmsg_multi_iter() {
        let test_multi_iter = |(reader, addr, sender, saddr): TestConfig| {
            let sent = TEST_NUM_MSGS + 10;
            for _ in 0..sent {
                let data = [0; PACKET_DATA_SIZE];
                sender.send_to(&data[..], &addr).unwrap();
            }

            let mut packets = vec![Packet::default(); TEST_NUM_MSGS];
            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(TEST_NUM_MSGS, recv);
            for packet in packets.iter().take(recv) {
                assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
                assert_eq!(packet.meta.addr(), saddr);
            }

            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(sent - TEST_NUM_MSGS, recv);
            for packet in packets.iter().take(recv) {
                assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
                assert_eq!(packet.meta.addr(), saddr);
            }
        };

        test_multi_iter(test_setup_reader_sender("127.0.0.1:0").unwrap());

        match test_setup_reader_sender("::1:0") {
            Ok(config) => test_multi_iter(config),
            Err(e) => warn!("Failed to configure IPv6: {:?}", e),
        }
    }

    #[test]
    pub fn test_recv_mmsg_multi_iter_timeout() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        reader.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
        reader.set_nonblocking(false).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = sender.local_addr().unwrap();
        let sent = TEST_NUM_MSGS;
        for _ in 0..sent {
            let data = [0; PACKET_DATA_SIZE];
            sender.send_to(&data[..], &addr).unwrap();
        }

        let start = Instant::now();
        let mut packets = vec![Packet::default(); TEST_NUM_MSGS];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(TEST_NUM_MSGS, recv);
        for packet in packets.iter().take(recv) {
            assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
            assert_eq!(packet.meta.addr(), saddr);
        }
        reader.set_nonblocking(true).unwrap();

        let _recv = recv_mmsg(&reader, &mut packets[..]);
        assert!(start.elapsed().as_secs() < 5);
    }

    #[test]
    pub fn test_recv_mmsg_multi_addrs() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let sender1 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr1 = sender1.local_addr().unwrap();
        let sent1 = TEST_NUM_MSGS - 1;

        let sender2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr2 = sender2.local_addr().unwrap();
        let sent2 = TEST_NUM_MSGS + 1;

        for _ in 0..sent1 {
            let data = [0; PACKET_DATA_SIZE];
            sender1.send_to(&data[..], &addr).unwrap();
        }

        for _ in 0..sent2 {
            let data = [0; PACKET_DATA_SIZE];
            sender2.send_to(&data[..], &addr).unwrap();
        }

        let mut packets = vec![Packet::default(); TEST_NUM_MSGS];

        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(TEST_NUM_MSGS, recv);
        for packet in packets.iter().take(sent1) {
            assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
            assert_eq!(packet.meta.addr(), saddr1);
        }
        for packet in packets.iter().skip(sent1).take(recv - sent1) {
            assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
            assert_eq!(packet.meta.addr(), saddr2);
        }

        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(sent1 + sent2 - TEST_NUM_MSGS, recv);
        for packet in packets.iter().take(recv) {
            assert_eq!(packet.meta.size, PACKET_DATA_SIZE);
            assert_eq!(packet.meta.addr(), saddr2);
        }
    }
}
