//! The `packet` module defines data structures and methods to pull data from the network.
use crate::recvmmsg::{recv_mmsg, NUM_RCVMMSGS};
use crate::result::{Error, Result};
use bincode;
use serde::Serialize;
use solana_metrics::counter::Counter;
pub use solana_sdk::packet::PACKET_DATA_SIZE;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

pub use crate::blob::*;

pub const NUM_PACKETS: usize = 1024 * 8;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub forward: bool,
    pub addr: [u16; 8],
    pub port: u16,
    pub v6: bool,
}

#[derive(Clone)]
#[repr(C)]
pub struct Packet {
    pub data: [u8; PACKET_DATA_SIZE],
    pub meta: Meta,
}

impl Packet {
    pub fn new(data: [u8; PACKET_DATA_SIZE], meta: Meta) -> Self {
        Self { data, meta }
    }
}

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Packet {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.meta.addr()
        )
    }
}

impl Default for Packet {
    fn default() -> Packet {
        Packet {
            data: unsafe { std::mem::uninitialized() },
            meta: Meta::default(),
        }
    }
}

impl PartialEq for Packet {
    fn eq(&self, other: &Packet) -> bool {
        self.meta == other.meta && self.data.as_ref() == other.data.as_ref()
    }
}

impl Meta {
    pub fn addr(&self) -> SocketAddr {
        if !self.v6 {
            let addr = [
                self.addr[0] as u8,
                self.addr[1] as u8,
                self.addr[2] as u8,
                self.addr[3] as u8,
            ];
            let ipv4: Ipv4Addr = From::<[u8; 4]>::from(addr);
            SocketAddr::new(IpAddr::V4(ipv4), self.port)
        } else {
            let ipv6: Ipv6Addr = From::<[u16; 8]>::from(self.addr);
            SocketAddr::new(IpAddr::V6(ipv6), self.port)
        }
    }

    pub fn set_addr(&mut self, a: &SocketAddr) {
        match *a {
            SocketAddr::V4(v4) => {
                let ip = v4.ip().octets();
                self.addr[0] = u16::from(ip[0]);
                self.addr[1] = u16::from(ip[1]);
                self.addr[2] = u16::from(ip[2]);
                self.addr[3] = u16::from(ip[3]);
                self.addr[4] = 0;
                self.addr[5] = 0;
                self.addr[6] = 0;
                self.addr[7] = 0;
                self.v6 = false;
            }
            SocketAddr::V6(v6) => {
                self.addr = v6.ip().segments();
                self.v6 = true;
            }
        }
        self.port = a.port();
    }
}

#[derive(Debug, Clone)]
pub struct Packets {
    pub packets: Vec<Packet>,
}

//auto derive doesn't support large arrays
impl Default for Packets {
    fn default() -> Packets {
        Packets {
            packets: Vec::with_capacity(NUM_RCVMMSGS),
        }
    }
}

impl Packets {
    pub fn new(packets: Vec<Packet>) -> Self {
        Self { packets }
    }

    pub fn set_addr(&mut self, addr: &SocketAddr) {
        for m in self.packets.iter_mut() {
            m.meta.set_addr(&addr);
        }
    }
}

impl Packets {
    pub fn recv_from(&mut self, socket: &UdpSocket) -> Result<usize> {
        let mut i = 0;
        //DOCUMENTED SIDE-EFFECT
        //Performance out of the IO without poll
        //  * block on the socket until it's readable
        //  * set the socket to non blocking
        //  * read until it fails
        //  * set it back to blocking before returning
        socket.set_nonblocking(false)?;
        trace!("receiving on {}", socket.local_addr().unwrap());
        loop {
            self.packets.resize(i + NUM_RCVMMSGS, Packet::default());
            match recv_mmsg(socket, &mut self.packets[i..]) {
                Err(_) if i > 0 => {
                    break;
                }
                Err(e) => {
                    trace!("recv_from err {:?}", e);
                    return Err(Error::IO(e));
                }
                Ok(npkts) => {
                    if i == 0 {
                        socket.set_nonblocking(true)?;
                    }
                    trace!("got {} packets", npkts);
                    i += npkts;
                    if npkts != NUM_RCVMMSGS || i >= 1024 {
                        break;
                    }
                }
            }
        }
        self.packets.truncate(i);
        inc_new_counter_info!("packets-recv_count", i);
        Ok(i)
    }

    pub fn send_to(&self, socket: &UdpSocket) -> Result<()> {
        for p in &self.packets {
            let a = p.meta.addr();
            socket.send_to(&p.data[..p.meta.size], &a)?;
        }
        Ok(())
    }
}

pub fn to_packets_chunked<T: Serialize>(xs: &[T], chunks: usize) -> Vec<Packets> {
    let mut out = vec![];
    for x in xs.chunks(chunks) {
        let mut p = Packets::default();
        p.packets.resize(x.len(), Packet::default());
        for (i, o) in x.iter().zip(p.packets.iter_mut()) {
            let mut wr = io::Cursor::new(&mut o.data[..]);
            bincode::serialize_into(&mut wr, &i).expect("serialize request");
            let len = wr.position() as usize;
            o.meta.size = len;
        }
        out.push(p);
    }
    out
}

pub fn to_packets<T: Serialize>(xs: &[T]) -> Vec<Packets> {
    to_packets_chunked(xs, NUM_PACKETS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::io::{self, Write};
    use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

    #[test]
    fn test_packets_set_addr() {
        // test that the address is actually being updated
        let send_addr = socketaddr!([127, 0, 0, 1], 123);
        let packets = vec![Packet::default()];
        let mut msgs = Packets { packets };
        msgs.set_addr(&send_addr);
        assert_eq!(SocketAddr::from(msgs.packets[0].meta.addr()), send_addr);
    }

    #[test]
    pub fn packet_send_recv() {
        solana_logger::setup();
        let recv_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = recv_socket.local_addr().unwrap();
        let send_socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = send_socket.local_addr().unwrap();
        let mut p = Packets::default();

        p.packets.resize(10, Packet::default());

        for m in p.packets.iter_mut() {
            m.meta.set_addr(&addr);
            m.meta.size = PACKET_DATA_SIZE;
        }
        p.send_to(&send_socket).unwrap();

        let recvd = p.recv_from(&recv_socket).unwrap();

        assert_eq!(recvd, p.packets.len());

        for m in p.packets {
            assert_eq!(m.meta.size, PACKET_DATA_SIZE);
            assert_eq!(m.meta.addr(), saddr);
        }
    }

    #[test]
    fn test_to_packets() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = system_transaction::create_user_account(&keypair, &keypair.pubkey(), 1, hash, 0);
        let rv = to_packets(&vec![tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), 1);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].packets.len(), 1);
    }

    #[test]
    pub fn debug_trait() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
    }
}
