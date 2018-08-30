//! The `nat` module assists with NAT traversal

extern crate reqwest;

use pnet_datalink as datalink;
use rand::{thread_rng, Rng};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

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

pub fn bind_in_range(range: (u16, u16)) -> io::Result<UdpSocket> {
    let (start, end) = range;
    let mut tries_left = end - start;
    loop {
        let rand_port = thread_rng().gen_range(start, end);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rand_port);

        match UdpSocket::bind(addr) {
            Result::Ok(val) => break Result::Ok(val),
            Result::Err(err) => if err.kind() != io::ErrorKind::AddrInUse || tries_left == 0 {
                return Err(err);
            },
        }
        tries_left -= 1;
    }
}

#[cfg(test)]
mod tests {
    use nat::parse_port_or_addr;

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
}
