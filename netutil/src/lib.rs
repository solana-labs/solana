//! The `netutil` module assists with networking
use nix::sys::socket::setsockopt;
use nix::sys::socket::sockopt::{ReuseAddr, ReusePort};
use rand::{thread_rng, Rng};
use socket2::{Domain, SockAddr, Socket, Type};
use std::io;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::os::unix::io::AsRawFd;
use std::time::Duration;

mod ip_echo_server;
pub use ip_echo_server::*;

/// A data type representing a public Udp socket
pub struct UdpSocketPair {
    pub addr: SocketAddr,    // Public address of the socket
    pub receiver: UdpSocket, // Locally bound socket that can receive from the public address
    pub sender: UdpSocket,   // Locally bound socket to send via public address
}

pub type PortRange = (u16, u16);

/// Determine the public IP address of this machine by asking an ip_echo_server at the given
/// address
pub fn get_public_ip_addr(ip_echo_server_addr: &SocketAddr) -> Result<IpAddr, String> {
    let mut data = Vec::new();

    let timeout = Duration::new(5, 0);
    TcpStream::connect_timeout(ip_echo_server_addr, timeout)
        .and_then(|mut stream| {
            stream
                .set_read_timeout(Some(Duration::new(10, 0)))
                .expect("set_read_timeout");
            stream.read_to_end(&mut data)
        })
        .and_then(|_| {
            bincode::deserialize(&data).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to deserialize: {:?}", err),
                )
            })
        })
        .map_err(|err| err.to_string())
}

pub fn parse_port_or_addr(optstr: Option<&str>, default_addr: SocketAddr) -> SocketAddr {
    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = default_addr;
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            default_addr
        }
    } else {
        default_addr
    }
}

pub fn parse_port_range(port_range: &str) -> Option<PortRange> {
    let ports: Vec<&str> = port_range.split('-').collect();
    if ports.len() != 2 {
        return None;
    }

    let start_port = ports[0].parse();
    let end_port = ports[1].parse();

    if start_port.is_err() || end_port.is_err() {
        return None;
    }
    let start_port = start_port.unwrap();
    let end_port = end_port.unwrap();
    if end_port < start_port {
        return None;
    }
    Some((start_port, end_port))
}

pub fn parse_host(host: &str) -> Result<IpAddr, String> {
    let ips: Vec<_> = (host, 0)
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .map(|socket_address| socket_address.ip())
        .collect();
    if ips.is_empty() {
        Err(format!("Unable to resolve host: {}", host))
    } else {
        Ok(ips[0])
    }
}

pub fn parse_host_port(host_port: &str) -> Result<SocketAddr, String> {
    let addrs: Vec<_> = host_port
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .collect();
    if addrs.is_empty() {
        Err(format!("Unable to resolve host: {}", host_port))
    } else {
        Ok(addrs[0])
    }
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

pub fn bind_in_range(range: PortRange) -> io::Result<(u16, UdpSocket)> {
    let sock = udp_socket(false)?;

    let (start, end) = range;
    let mut tries_left = end - start;
    let mut rand_port = thread_rng().gen_range(start, end);
    loop {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rand_port);

        match sock.bind(&SockAddr::from(addr)) {
            Ok(_) => {
                let sock = sock.into_udp_socket();
                break Result::Ok((sock.local_addr().unwrap().port(), sock));
            }
            Err(err) => {
                if tries_left == 0 {
                    return Err(err);
                }
            }
        }
        rand_port += 1;
        if rand_port == end {
            rand_port = start;
        }
        tries_left -= 1;
    }
}

// binds many sockets to the same port in a range
pub fn multi_bind_in_range(range: PortRange, num: usize) -> io::Result<(u16, Vec<UdpSocket>)> {
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

pub fn find_available_port_in_range(range: PortRange) -> io::Result<u16> {
    let (start, end) = range;
    let mut tries_left = end - start;
    let mut rand_port = thread_rng().gen_range(start, end);
    loop {
        match TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            rand_port,
        )) {
            Ok(_) => {
                break Ok(rand_port);
            }
            Err(err) => {
                if tries_left == 0 {
                    return Err(err);
                }
            }
        }
        rand_port += 1;
        if rand_port == end {
            rand_port = start;
        }
        tries_left -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port_or_addr() {
        let p1 = parse_port_or_addr(Some("9000"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p1.port(), 9000);
        let p2 = parse_port_or_addr(Some("127.0.0.1:7000"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p2.port(), 7000);
        let p2 = parse_port_or_addr(Some("hi there"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p2.port(), 1);
        let p3 = parse_port_or_addr(None, SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p3.port(), 1);
    }

    #[test]
    fn test_parse_port_range() {
        assert_eq!(parse_port_range("garbage"), None);
        assert_eq!(parse_port_range("1-"), None);
        assert_eq!(parse_port_range("1-2"), Some((1, 2)));
        assert_eq!(parse_port_range("1-2-3"), None);
        assert_eq!(parse_port_range("2-1"), None);
    }

    #[test]
    fn test_parse_host() {
        parse_host("localhost:1234").unwrap_err();
        parse_host("localhost").unwrap();
        parse_host("127.0.0.0:1234").unwrap_err();
        parse_host("127.0.0.0").unwrap();
    }

    #[test]
    fn test_parse_host_port() {
        parse_host_port("localhost:1234").unwrap();
        parse_host_port("localhost").unwrap_err();
        parse_host_port("127.0.0.0:1234").unwrap();
        parse_host_port("127.0.0.0").unwrap_err();
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
    fn test_find_available_port_in_range() {
        assert_eq!(find_available_port_in_range((3000, 3001)).unwrap(), 3000);
        let port = find_available_port_in_range((3000, 3050)).unwrap();
        assert!(3000 <= port && port < 3050);
    }
}
