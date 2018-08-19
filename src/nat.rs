//! The `nat` module assists with NAT traversal

extern crate reqwest;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use rand::{thread_rng, Rng};
use std::io;

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

pub fn udp_random_bind(start: u16, end: u16, tries: u32) -> io::Result<UdpSocket> {
    let mut count = 0;
    loop {
        count += 1;

        let rand_port = thread_rng().gen_range(start, end);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rand_port);

        match UdpSocket::bind(addr) {
            Result::Ok(val) => break Result::Ok(val),
            Result::Err(err) => if err.kind() != io::ErrorKind::AddrInUse || count >= tries {
                return Err(err);
            },
        }
    }
}
