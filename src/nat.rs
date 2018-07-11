//! The `nat` module assists with NAT traversal

extern crate futures;
extern crate p2p;
extern crate rand;
extern crate reqwest;
extern crate tokio_core;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use self::futures::Future;
use self::p2p::UdpSocketExt;
use rand::{thread_rng, Rng};
use std::env;
use std::io;
use std::str;

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

/// Binds a private Udp address to a public address using UPnP if possible
pub fn udp_public_bind(label: &str, startport: u16, endport: u16) -> UdpSocketPair {
    let private_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let handle = core.handle();
    let mc = p2p::P2p::default();
    let res = core.run({
        tokio_core::net::UdpSocket::bind_public(&private_addr, &handle, &mc)
            .map_err(|e| {
                info!("Failed to bind public socket for {}: {}", label, e);
            })
            .and_then(|(socket, public_addr)| Ok((public_addr, socket.local_addr().unwrap())))
    });

    match res {
        Ok((public_addr, local_addr)) => {
            info!(
                "Using local address {} mapped to UPnP public address {} for {}",
                local_addr, public_addr, label
            );

            // NAT should now be forwarding inbound packets directed at
            // |public_addr| to the local |receiver| socket...
            let receiver = UdpSocket::bind(local_addr).unwrap();

            // TODO: try to autodetect a broken NAT (issue #496)
            let sender = if env::var("BROKEN_NAT").is_err() {
                receiver.try_clone().unwrap()
            } else {
                // ... however for outbound packets, some NATs *will not* rewrite the
                // source port from |receiver.local_addr().port()| to |public_addr.port()|.
                // This is currently a problem when talking with a fullnode as it
                // assumes it can send UDP packets back at the source.  This hits the
                // NAT as a datagram for |receiver.local_addr().port()| on the NAT's public
                // IP, which the NAT promptly discards.  As a short term hack, create a
                // local UDP socket, |sender|, with the same port as |public_addr.port()|.
                //
                // TODO: Remove the |sender| socket and deal with the downstream changes to
                //       the UDP signalling
                let mut local_addr_sender = local_addr;
                local_addr_sender.set_port(public_addr.port());
                UdpSocket::bind(local_addr_sender).unwrap()
            };

            UdpSocketPair {
                addr: public_addr,
                receiver,
                sender,
            }
        }
        Err(_) => {
            let sender = udp_random_bind(startport, endport, 5).unwrap();
            let local_addr = sender.local_addr().unwrap();
            info!("Using local address {} for {}", local_addr, label);
            UdpSocketPair {
                addr: private_addr,
                receiver: sender.try_clone().unwrap(),
                sender,
            }
        }
    }
}
