//! The `netutil` module assists with networking

#[cfg(target_os = "linux")]
use libc::{
    c_void, iovec, mmsghdr, recvmmsg, sockaddr_in, socklen_t, time_t, timespec, MSG_WAITFORONE,
};
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::{ReuseAddr, ReusePort};
#[cfg(target_os = "linux")]
use nix::sys::socket::InetAddr;
use packet::Packet;
use pnet_datalink as datalink;
use rand::{thread_rng, Rng};
use reqwest;
use socket2::{Domain, SockAddr, Socket, Type};
#[cfg(target_os = "linux")]
use std::cmp;
use std::io;
#[cfg(target_os = "linux")]
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::os::unix::io::AsRawFd;

pub const NUM_RCVMMSGS: usize = 16;

/// A data type representing a public Udp socket
pub struct UdpSocketPair {
    pub addr: SocketAddr,    // Public address of the socket
    pub receiver: UdpSocket, // Locally bound socket that can receive from the public address
    pub sender: UdpSocket,   // Locally bound socket to send via public address
}

/// Tries to determine the public IP address of this machine
pub fn get_public_ip_addr() -> Result<IpAddr, String> {
    let body = reqwest::get("http://ifconfig.co/ip")
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())?;

    match body.lines().next() {
        Some(ip) => Result::Ok(ip.parse().unwrap()),
        None => Result::Err("Empty response body".to_string()),
    }
}

pub fn parse_port_or_addr(optstr: Option<&str>, default_port: u16) -> SocketAddr {
    let daddr = SocketAddr::from(([0, 0, 0, 0], default_port));

    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = daddr;
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            daddr
        }
    } else {
        daddr
    }
}

pub fn get_ip_addr() -> Option<IpAddr> {
    for iface in datalink::interfaces() {
        for p in iface.ips {
            if !p.ip().is_loopback() && !p.ip().is_multicast() {
                match p.ip() {
                    IpAddr::V4(addr) => {
                        if !addr.is_link_local() {
                            return Some(p.ip());
                        }
                    }
                    IpAddr::V6(_addr) => {
                        // Select an ipv6 address if the config is selected
                        #[cfg(feature = "ipv6")]
                        {
                            return Some(p.ip());
                        }
                    }
                }
            }
        }
    }
    None
}

fn udp_socket(reuseaddr: bool) -> io::Result<Socket> {
    let sock = Socket::new(Domain::ipv4(), Type::dgram(), None)?;
    let sock_fd = sock.as_raw_fd();

    if reuseaddr {
        // best effort, i.e. ignore errors here, we'll get the failure in caller
        setsockopt(sock_fd, ReusePort, &true).ok();
        setsockopt(sock_fd, ReuseAddr, &true).ok();
    }

    Ok(sock)
}

pub fn bind_in_range(range: (u16, u16)) -> io::Result<(u16, UdpSocket)> {
    let sock = udp_socket(false)?;

    let (start, end) = range;
    let mut tries_left = end - start;
    loop {
        let rand_port = thread_rng().gen_range(start, end);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rand_port);

        match sock.bind(&SockAddr::from(addr)) {
            Ok(_) => {
                let sock = sock.into_udp_socket();
                break Result::Ok((sock.local_addr().unwrap().port(), sock));
            }
            Err(err) => if err.kind() != io::ErrorKind::AddrInUse || tries_left == 0 {
                return Err(err);
            },
        }
        tries_left -= 1;
    }
}

// binds many sockets to the same port in a range
pub fn multi_bind_in_range(range: (u16, u16), num: usize) -> io::Result<(u16, Vec<UdpSocket>)> {
    let mut sockets = Vec::with_capacity(num);

    let port = {
        let (port, _) = bind_in_range(range)?;
        port
    }; // drop the probe, port should be available... briefly.

    for _ in 0..num {
        sockets.push(bind_to(port, true)?);
    }
    Ok((port, sockets))
}

pub fn bind_to(port: u16, reuseaddr: bool) -> io::Result<UdpSocket> {
    let sock = udp_socket(reuseaddr)?;

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);

    match sock.bind(&SockAddr::from(addr)) {
        Ok(_) => Result::Ok(sock.into_udp_socket()),
        Err(err) => Err(err),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn recv_mmsg(socket: &UdpSocket, packets: &mut [Packet]) -> io::Result<usize> {
    let mut i = 0;
    socket.set_nonblocking(false)?;
    for n in 0..NUM_RCVMMSGS {
        let p = &mut packets[n];
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
pub fn recv_mmsg(sock: &UdpSocket, packets: &mut [Packet]) -> io::Result<usize> {
    let mut hdrs: [mmsghdr; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let mut iovs: [iovec; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let mut addr: [sockaddr_in; NUM_RCVMMSGS] = unsafe { mem::zeroed() };
    let addrlen = mem::size_of_val(&addr) as socklen_t;

    let sock_fd = sock.as_raw_fd();

    let count = cmp::min(iovs.len(), packets.len());

    for i in 0..count {
        iovs[i].iov_base = packets[i].data.as_mut_ptr() as *mut c_void;
        iovs[i].iov_len = packets[i].data.len();

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
                    let mut p = &mut packets[i];
                    p.meta.size = hdrs[i].msg_len as usize;
                    let inet_addr = InetAddr::V4(addr[i]);
                    p.meta.set_addr(&inet_addr.to_std());
                }
                n as usize
            }
        };

    Ok(npkts)
}

#[cfg(test)]
mod tests {
    use netutil::*;
    use packet::PACKET_DATA_SIZE;

    #[test]
    fn test_parse_port_or_addr() {
        let p1 = parse_port_or_addr(Some("9000"), 1);
        assert_eq!(p1.port(), 9000);
        let p2 = parse_port_or_addr(Some("127.0.0.1:7000"), 1);
        assert_eq!(p2.port(), 7000);
        let p2 = parse_port_or_addr(Some("hi there"), 1);
        assert_eq!(p2.port(), 1);
        let p3 = parse_port_or_addr(None, 1);
        assert_eq!(p3.port(), 1);
    }

    #[test]
    fn test_bind() {
        assert_eq!(bind_in_range((2000, 2001)).unwrap().0, 2000);
        let x = bind_to(2002, true).unwrap();
        let y = bind_to(2002, true).unwrap();
        assert_eq!(
            x.local_addr().unwrap().port(),
            y.local_addr().unwrap().port()
        );
        let (port, v) = multi_bind_in_range((2010, 2110), 10).unwrap();
        for sock in &v {
            assert_eq!(port, sock.local_addr().unwrap().port());
        }
    }

    #[test]
    #[should_panic]
    fn test_bind_in_range_nil() {
        let _ = bind_in_range((2000, 2000));
    }

    #[test]
    pub fn test_recv_mmsg_one_iter() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = sender.local_addr().unwrap();
        let sent = NUM_RCVMMSGS - 1;
        for _ in 0..sent {
            let data = [0; PACKET_DATA_SIZE];
            sender.send_to(&data[..], &addr).unwrap();
        }

        let mut packets = vec![Packet::default(); NUM_RCVMMSGS];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(sent, recv);
        for i in 0..recv {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr);
        }
    }

    #[test]
    pub fn test_recv_mmsg_multi_iter() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = sender.local_addr().unwrap();
        let sent = NUM_RCVMMSGS + 10;
        for _ in 0..sent {
            let data = [0; PACKET_DATA_SIZE];
            sender.send_to(&data[..], &addr).unwrap();
        }

        let mut packets = vec![Packet::default(); NUM_RCVMMSGS * 2];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(NUM_RCVMMSGS, recv);
        for i in 0..recv {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr);
        }

        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(sent - NUM_RCVMMSGS, recv);
        for i in 0..recv {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr);
        }
    }

    #[test]
    pub fn test_recv_mmsg_multi_addrs() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let sender1 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr1 = sender1.local_addr().unwrap();
        let sent1 = NUM_RCVMMSGS - 1;

        let sender2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr2 = sender2.local_addr().unwrap();
        let sent2 = NUM_RCVMMSGS + 1;

        for _ in 0..sent1 {
            let data = [0; PACKET_DATA_SIZE];
            sender1.send_to(&data[..], &addr).unwrap();
        }

        for _ in 0..sent2 {
            let data = [0; PACKET_DATA_SIZE];
            sender2.send_to(&data[..], &addr).unwrap();
        }

        let mut packets = vec![Packet::default(); NUM_RCVMMSGS * 2];

        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(NUM_RCVMMSGS, recv);
        for i in 0..sent1 {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr1);
        }

        for i in sent1..recv {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr2);
        }

        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap();
        assert_eq!(sent1 + sent2 - NUM_RCVMMSGS, recv);
        for i in 0..recv {
            assert_eq!(packets[i].meta.size, PACKET_DATA_SIZE);
            assert_eq!(packets[i].meta.addr(), saddr2);
        }
    }
}
