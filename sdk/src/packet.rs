use crate::clock::Slot;
use bincode::Result;
use libc::{iovec, sockaddr_in, sockaddr_in6, sockaddr_storage, AF_INET, AF_INET6, sa_family_t};
use serde::Serialize;
use std::{
    mem,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    {fmt, io},
};

/// Maximum over-the-wire size of a Transaction
///   1280 is IPv6 minimum MTU
///   40 bytes is the size of the IPv6 header
///   8 bytes is the size of the fragment header
pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;

#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct AddrStorage {
    pub storage: sockaddr_storage,
    pub iov: iovec,
}

impl Default for AddrStorage {
    fn default() -> AddrStorage {
        let mut a = AddrStorage {
            storage: unsafe { mem::MaybeUninit::uninit().assume_init() },
            iov: unsafe { mem::zeroed() },
        };
        let unspecified_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        a.set_addr(&unspecified_v4);
        a
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub forward: bool,
    pub repair: bool,
    pub discard: bool,
    //    pub addr_storage: AddrStorage,
    //    pub addr: [u16; 8],
    //    pub port: u16,
    //    pub v6: bool,
    pub seed: [u8; 32],
    pub slot: Slot,
    pub is_tracer_tx: bool,
}

#[derive(Clone)]
#[repr(C)]
pub struct Packet {
    pub data: [u8; PACKET_DATA_SIZE],
    pub meta: Meta,
    pub addr: AddrStorage,
}

impl Packet {
    pub fn new(data: [u8; PACKET_DATA_SIZE], meta: Meta) -> Self {
        Self {
            data,
            meta,
            addr: AddrStorage::default(),
        }
    }

    pub fn from_data<T: Serialize>(dest: Option<&SocketAddr>, data: T) -> Result<Self> {
        let mut packet = Packet::default();
        Self::populate_packet(&mut packet, dest, &data)?;
        Ok(packet)
    }

    pub fn populate_packet<T: Serialize>(
        packet: &mut Packet,
        dest: Option<&SocketAddr>,
        data: &T,
    ) -> Result<()> {
        let mut wr = io::Cursor::new(&mut packet.data[..]);
        bincode::serialize_into(&mut wr, data)?;
        let len = wr.position() as usize;
        packet.meta.size = len;
        if let Some(dest) = dest {
            packet.addr.set_addr(dest);
//            packet.meta.set_addr(dest);
        }
        Ok(())
    }
}

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Packet {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.addr.addr()
        )
    }
}

#[allow(clippy::uninit_assumed_init)]
impl Default for Packet {
    fn default() -> Packet {
        // Ipv6Addr::UNSPECIFIED
        Packet {
            data: unsafe { std::mem::MaybeUninit::uninit().assume_init() },
            meta: Meta::default(),
            addr: AddrStorage::default(),
        }
    }
}

impl PartialEq for Packet {
    fn eq(&self, other: &Packet) -> bool {
        let self_data: &[u8] = self.data.as_ref();
        let other_data: &[u8] = other.data.as_ref();
        self.meta == other.meta && self_data[..self.meta.size] == other_data[..self.meta.size]
    }
}

impl AddrStorage {
    pub fn addr(&self) -> SocketAddr {
        if self.storage.ss_family == AF_INET as sa_family_t {
            unsafe {
                let p: *const sockaddr_in = &self.storage as *const _ as *const _;
                let port = (*p).sin_port;
                let ip = (*p).sin_addr.s_addr;
                let ipv4: Ipv4Addr = From::<u32>::from(ip);
                SocketAddr::new(IpAddr::V4(ipv4), port)
            }
        } else if self.storage.ss_family == AF_INET6 as sa_family_t {
            unsafe {
                let p: *const sockaddr_in6 = &self.storage as *const _ as *const _;
                let port = (*p).sin6_port;
                let ip: [u8; 16] = (*p).sin6_addr.s6_addr;
                let ipv6: Ipv6Addr = From::<[u8; 16]>::from(ip);
                SocketAddr::new(IpAddr::V6(ipv6), port)
            }
        } else {
            panic!("bad addr");
        }
    }

    pub fn set_addr(&mut self, a: &SocketAddr) {
        match *a {
            SocketAddr::V4(v4) => {
                self.storage.ss_family = libc::AF_INET as sa_family_t;
                unsafe {
                    //let mut x: *mut sockaddr_storage = &mut self.storage;
                    //let mut y: *mut sockaddr_in = x as *mut _ as *mut _;
                    let mut p: *mut sockaddr_in = &mut self.storage as *mut _ as *mut _;
                    (*p).sin_port = v4.port();
                    (*p).sin_addr.s_addr = From::<Ipv4Addr>::from(*v4.ip());
                }
            }
            SocketAddr::V6(v6) => {
                self.storage.ss_family = libc::AF_INET6 as sa_family_t;
                unsafe {
                    //let mut x: *mut sockaddr_storage = &mut self.storage;
                    //let mut y: *mut sockaddr_in6 = x as *mut _ as *mut _;
                    let mut p: *mut sockaddr_in6 = &mut self.storage as *mut _ as *mut _;
                    (*p).sin6_port = v6.port();
                    (*p).sin6_addr.s6_addr = v6.ip().octets();
                }
            }
        }
    }
}

/*
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
*/
