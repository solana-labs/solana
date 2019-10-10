//! The `sendmmsg` module provides sendmmsg() API implementation

use crate::packet::Packet;
use std::io;
use std::net::{SocketAddr, UdpSocket};

#[cfg(not(target_os = "linux"))]
pub fn send_mmsg(sock: &UdpSocket, packets: &mut [Packet]) -> io::Result<usize> {
    let count = packets.len();
    for p in packets {
        let a = p.meta.addr();
        sock.send_to(&p.data[..p.meta.size], &a)?;
    }

    Ok(count)
}

#[cfg(target_os = "linux")]
use libc::{iovec, mmsghdr, sockaddr_in, sockaddr_in6};

#[cfg(target_os = "linux")]
fn mmsghdr_for_packet(
    packet: &mut Packet,
    dest: &SocketAddr,
    index: usize,
    addr_in_len: u32,
    addr_in6_len: u32,
    iovs: &mut Vec<iovec>,
    addr_in: &mut Vec<sockaddr_in>,
    addr_in6: &mut Vec<sockaddr_in6>,
) -> mmsghdr {
    use libc::c_void;
    use nix::sys::socket::InetAddr;
    use std::mem;

    iovs.push(iovec {
        iov_base: packet.data.as_mut_ptr() as *mut c_void,
        iov_len: packet.data.len(),
    });

    let mut hdr: mmsghdr = unsafe { mem::zeroed() };
    hdr.msg_hdr.msg_iov = &mut iovs[index];
    hdr.msg_hdr.msg_iovlen = 1;
    hdr.msg_len = packet.data.len() as u32;

    match InetAddr::from_std(dest) {
        InetAddr::V4(addr) => {
            addr_in.insert(index, addr);
            hdr.msg_hdr.msg_name = &mut addr_in[index] as *mut _ as *mut _;
            hdr.msg_hdr.msg_namelen = addr_in_len;
        }
        InetAddr::V6(addr) => {
            addr_in6.insert(index, addr);
            hdr.msg_hdr.msg_name = &mut addr_in6[index] as *mut _ as *mut _;
            hdr.msg_hdr.msg_namelen = addr_in6_len;
        }
    };
    hdr
}
#[cfg(target_os = "linux")]
pub fn send_mmsg(sock: &UdpSocket, packets: &mut [Packet]) -> io::Result<usize> {
    use libc::{sendmmsg, socklen_t};
    use std::mem;
    use std::os::unix::io::AsRawFd;

    // The vectors are allocated with capacity, as later code inserts elements
    // at specific indices, and uses the address of the vector index in hdrs
    let mut iovs: Vec<iovec> = Vec::with_capacity(packets.len());
    let mut addr_in: Vec<sockaddr_in> = Vec::with_capacity(packets.len());
    let mut addr_in6: Vec<sockaddr_in6> = Vec::with_capacity(packets.len());

    let addr_in_len = mem::size_of_val(&addr_in) as socklen_t;
    let addr_in6_len = mem::size_of_val(&addr_in6) as socklen_t;
    let sock_fd = sock.as_raw_fd();

    let mut hdrs: Vec<mmsghdr> = packets
        .iter_mut()
        .enumerate()
        .map(|(i, packet)| {
            mmsghdr_for_packet(
                packet,
                &packet.meta.addr(),
                i,
                addr_in_len as u32,
                addr_in6_len as u32,
                &mut iovs,
                &mut addr_in,
                &mut addr_in6,
            )
        })
        .collect();

    let npkts = match unsafe { sendmmsg(sock_fd, &mut hdrs[0], packets.len() as u32, 0) } {
        -1 => return Err(io::Error::last_os_error()),
        n => n as usize,
    };
    Ok(npkts)
}

#[cfg(not(target_os = "linux"))]
pub fn multicast(
    sock: &UdpSocket,
    packet: &mut Packet,
    dests: &[&SocketAddr],
) -> io::Result<usize> {
    let count = dests.len();
    for a in dests {
        sock.send_to(&packet.data[..packet.meta.size], a)?;
    }

    Ok(count)
}

#[cfg(target_os = "linux")]
pub fn multicast(
    sock: &UdpSocket,
    packet: &mut Packet,
    dests: &[&SocketAddr],
) -> io::Result<usize> {
    use libc::{sendmmsg, socklen_t};
    use std::mem;
    use std::os::unix::io::AsRawFd;

    // The vectors are allocated with capacity, as later code inserts elements
    // at specific indices, and uses the address of the vector index in hdrs
    let mut iovs: Vec<iovec> = Vec::with_capacity(dests.len());
    let mut addr_in: Vec<sockaddr_in> = Vec::with_capacity(dests.len());
    let mut addr_in6: Vec<sockaddr_in6> = Vec::with_capacity(dests.len());

    let addr_in_len = mem::size_of_val(&addr_in) as socklen_t;
    let addr_in6_len = mem::size_of_val(&addr_in6) as socklen_t;
    let sock_fd = sock.as_raw_fd();

    let mut hdrs: Vec<mmsghdr> = dests
        .iter()
        .enumerate()
        .map(|(i, dest)| {
            mmsghdr_for_packet(
                packet,
                dest,
                i,
                addr_in_len as u32,
                addr_in6_len as u32,
                &mut iovs,
                &mut addr_in,
                &mut addr_in6,
            )
        })
        .collect();

    let npkts = match unsafe { sendmmsg(sock_fd, &mut hdrs[0], dests.len() as u32, 0) } {
        -1 => return Err(io::Error::last_os_error()),
        n => n as usize,
    };
    Ok(npkts)
}

#[cfg(test)]
mod tests {
    use crate::packet::Packet;
    use crate::recvmmsg::recv_mmsg;
    use crate::sendmmsg::{multicast, send_mmsg};
    use std::net::UdpSocket;

    #[test]
    pub fn test_send_mmsg_one_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let mut packets: Vec<Packet> = (0..32)
            .map(|_| {
                let mut p = Packet::default();
                p.meta.set_addr(&addr);
                p
            })
            .collect();

        let sent = send_mmsg(&sender, &mut packets);
        assert_matches!(sent, Ok(32));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(32, recv);
    }

    #[test]
    pub fn test_send_mmsg_multi_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let mut packets: Vec<Packet> = (0..32)
            .map(|i| {
                let mut p = Packet::default();
                if i < 16 {
                    p.meta.set_addr(&addr);
                } else {
                    p.meta.set_addr(&addr2);
                }
                p
            })
            .collect();

        let sent = send_mmsg(&sender, &mut packets);
        assert_matches!(sent, Ok(32));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(16, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader2, &mut packets[..]).unwrap().1;
        assert_eq!(16, recv);
    }

    #[test]
    pub fn test_multicast_msg() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let reader3 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr3 = reader3.local_addr().unwrap();

        let reader4 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr4 = reader4.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let mut packet = Packet::default();

        let sent = multicast(&sender, &mut packet, &[&addr, &addr2, &addr3, &addr4]);
        assert_matches!(sent, Ok(4));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader2, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader3, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader4, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);
    }
}
