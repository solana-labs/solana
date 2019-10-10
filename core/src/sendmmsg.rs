//! The `sendmmsg` module provides sendmmsg() API implementation

use crate::packet::Packet;
use std::io;
use std::net::UdpSocket;

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
pub fn send_mmsg(sock: &UdpSocket, packets: &mut [Packet]) -> io::Result<usize> {
    use libc::{c_void, iovec, mmsghdr, sendmmsg, sockaddr_in, sockaddr_in6, socklen_t};
    use nix::sys::socket::InetAddr;
    use std::mem;
    use std::os::unix::io::AsRawFd;

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
            iovs.push(iovec {
                iov_base: packet.data.as_mut_ptr() as *mut c_void,
                iov_len: packet.data.len(),
            });

            let mut hdr: mmsghdr = unsafe { mem::zeroed() };
            hdr.msg_hdr.msg_iov = &mut iovs[i];
            hdr.msg_hdr.msg_iovlen = 1;
            hdr.msg_len = packet.data.len() as u32;
            match InetAddr::from_std(&packet.meta.addr()) {
                InetAddr::V4(addr) => {
                    addr_in.insert(i, addr);
                    hdr.msg_hdr.msg_name = &mut addr_in[i] as *mut _ as *mut _;
                    hdr.msg_hdr.msg_namelen = addr_in_len;
                }
                InetAddr::V6(addr) => {
                    addr_in6.insert(i, addr);
                    hdr.msg_hdr.msg_name = &mut addr_in6[i] as *mut _ as *mut _;
                    hdr.msg_hdr.msg_namelen = addr_in6_len;
                }
            };
            hdr
        })
        .collect();

    let npkts = match unsafe { sendmmsg(sock_fd, &mut hdrs[0], packets.len() as u32, 0) } {
        -1 => return Err(io::Error::last_os_error()),
        n => n as usize,
    };
    Ok(npkts)
}

#[cfg(test)]
mod tests {
    use crate::packet::Packet;
    use crate::recvmmsg::recv_mmsg;
    use crate::sendmmsg::send_mmsg;
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
}
