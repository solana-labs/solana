//! The `recvmmsg` module provides recvmmsg() API implementation

use crate::packet::Meta;
use std::cmp;
use std::io;
use std::net::UdpSocket;

pub const NUM_RCVMMSGS: usize = 16;

pub trait RecvM: Clone + Default {
    const CAPACITY: usize;

    fn meta(&self) -> &Meta;
    fn meta_mut(&mut self) -> &mut Meta;

    fn content(&self) -> &[u8];
    fn content_mut(&mut self) -> &mut [u8];
}

#[cfg(not(target_os = "linux"))]
pub fn recv_mmsg<M: RecvM>(socket: &UdpSocket, packets: &mut [M]) -> io::Result<usize> {
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
pub fn recv_mmsg<M: RecvM>(sock: &UdpSocket, packets: &mut [M]) -> io::Result<usize> {
    use libc::{
        c_void, iovec, mmsghdr, recvmmsg, sockaddr_in, socklen_t, time_t, timespec, MSG_WAITFORONE,
    };
    use nix::sys::socket::InetAddr;
    use std::mem;
    use std::os::unix::io::AsRawFd;

    let mut hdrs: [mmsghdr; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let mut iovs: [iovec; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let mut addr: [sockaddr_in; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let addrlen = mem::size_of_val(&addr) as socklen_t;

    let sock_fd = sock.as_raw_fd();

    let count = cmp::min(iovs.len(), packets.len());

    for i in 0..count {
        iovs[i].iov_base = packets[i].content_mut().as_mut_ptr() as *mut c_void;
        iovs[i].iov_len = packets[i].content().len();

        hdrs[i].msg_hdr.msg_name = &mut addr[i] as *mut _ as *mut _;
        hdrs[i].msg_hdr.msg_namelen = addrlen;
        hdrs[i].msg_hdr.msg_iov = &mut iovs[i];
        hdrs[i].msg_hdr.msg_iovlen = 1;
    }
    let mut ts = timespec {
        tv_sec: 1 as time_t,
        tv_nsec: 0,
    };

    let npkts =
        match unsafe { recvmmsg(sock_fd, &mut hdrs[0], count as u32, MSG_WAITFORONE, &mut ts) } {
            -1 => return Err(io::Error::last_os_error()),
            n => {
                for i in 0..n as usize {
                    let meta = packets[i].meta_mut();
                    meta.size = hdrs[i].msg_len as usize;
                    let inet_addr = InetAddr::V4(addr[i]);
                    meta.set_addr(&inet_addr.to_std());
                }
                n as usize
            }
        };

    Ok(npkts)
}

#[cfg(test)]
mod tests {
    use crate::packet::{Blob, Packet};
    use crate::recvmmsg::*;
    use std::time::{Duration, Instant};

    #[test]
    pub fn test_recv_mmsg_one_iter() {
        fn helper<M: RecvM>() {
            let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let addr = reader.local_addr().unwrap();
            let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let saddr = sender.local_addr().unwrap();
            let sent = NUM_RCVMMSGS - 1;

            for _ in 0..sent {
                let data = vec![0; M::CAPACITY];
                sender.send_to(&data[..], &addr).unwrap();
            }

            let mut packets = vec![M::default(); NUM_RCVMMSGS];
            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(sent, recv);
            for i in 0..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr);
            }
        }

        helper::<Packet>();
        helper::<Blob>();
    }

    #[test]
    pub fn test_recv_mmsg_multi_iter() {
        fn helper<M: RecvM>() {
            let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let addr = reader.local_addr().unwrap();
            let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let saddr = sender.local_addr().unwrap();
            let sent = NUM_RCVMMSGS + 10;
            for _ in 0..sent {
                let data = vec![0; M::CAPACITY];
                sender.send_to(&data[..], &addr).unwrap();
            }

            let mut packets = vec![M::default(); NUM_RCVMMSGS * 2];
            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(NUM_RCVMMSGS, recv);
            for i in 0..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr);
            }

            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(sent - NUM_RCVMMSGS, recv);
            for i in 0..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr);
            }
        }

        helper::<Packet>();
        helper::<Blob>();
    }

    #[test]
    pub fn test_recv_mmsg_multi_iter_timeout() {
        fn helper<M: RecvM>() {
            let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let addr = reader.local_addr().unwrap();
            reader.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
            reader.set_nonblocking(false).unwrap();
            let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let saddr = sender.local_addr().unwrap();
            let sent = NUM_RCVMMSGS;
            for _ in 0..sent {
                let data = vec![0; M::CAPACITY];
                sender.send_to(&data[..], &addr).unwrap();
            }

            let start = Instant::now();
            let mut packets = vec![M::default(); NUM_RCVMMSGS * 2];
            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(NUM_RCVMMSGS, recv);
            for i in 0..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr);
            }
            reader.set_nonblocking(true).unwrap();

            let _recv = recv_mmsg(&reader, &mut packets[..]);
            assert!(start.elapsed().as_secs() < 5);
        }

        helper::<Packet>();
        helper::<Blob>();
    }

    #[test]
    pub fn test_recv_mmsg_multi_addrs() {
        fn helper<M: RecvM>() {
            let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let addr = reader.local_addr().unwrap();

            let sender1 = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let saddr1 = sender1.local_addr().unwrap();
            let sent1 = NUM_RCVMMSGS - 1;

            let sender2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
            let saddr2 = sender2.local_addr().unwrap();
            let sent2 = NUM_RCVMMSGS + 1;

            for _ in 0..sent1 {
                let data = vec![0; M::CAPACITY];
                sender1.send_to(&data[..], &addr).unwrap();
            }

            for _ in 0..sent2 {
                let data = vec![0; M::CAPACITY];
                sender2.send_to(&data[..], &addr).unwrap();
            }

            let mut packets = vec![M::default(); NUM_RCVMMSGS * 2];

            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(NUM_RCVMMSGS, recv);
            for i in 0..sent1 {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr1);
            }

            for i in sent1..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr2);
            }

            let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
            assert_eq!(sent1 + sent2 - NUM_RCVMMSGS, recv);
            for i in 0..recv {
                assert_eq!(packets[i].meta().size, M::CAPACITY);
                assert_eq!(packets[i].meta().addr(), saddr2);
            }
        }

        helper::<Packet>();
        helper::<Blob>();
    }
}
