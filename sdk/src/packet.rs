use crate::clock::Slot;
use bincode::Result;
use libc::{sa_family_t, sockaddr_in, sockaddr_in6, sockaddr_storage, AF_INET, AF_INET6};
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
pub struct Meta {
    pub addr: sockaddr_storage,
    pub seed: [u8; 32],
    pub slot: Slot,
    pub size: usize,
    pub forward: bool,
    pub repair: bool,
    pub discard: bool,
    pub is_tracer_tx: bool,
}

impl Default for Meta {
    fn default() -> Meta {
        let mut m = Meta {
            addr: unsafe { mem::zeroed() },
            seed: unsafe { mem::zeroed() },
            slot: Slot::default(),
            size: usize::default(),
            forward: bool::default(),
            repair: bool::default(),
            discard: bool::default(),
            is_tracer_tx: bool::default(),
        };
        let unspecified_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        m.set_addr(&unspecified_v4);
        m
    }
}

impl Meta {
    pub fn addr(&self) -> SocketAddr {
        if self.addr.ss_family == AF_INET as sa_family_t {
            unsafe {
                let p: *const sockaddr_in = &self.addr as *const _ as *const _;
                let port = (*p).sin_port;
                let ip = (*p).sin_addr.s_addr;
                let ipv4: Ipv4Addr = From::<u32>::from(ip);
                SocketAddr::new(IpAddr::V4(ipv4), port)
            }
        } else if self.addr.ss_family == AF_INET6 as sa_family_t {
            unsafe {
                let p: *const sockaddr_in6 = &self.addr as *const _ as *const _;
                let port = (*p).sin6_port;
                let ip: [u8; 16] = (*p).sin6_addr.s6_addr;
                let ipv6: Ipv6Addr = From::<[u8; 16]>::from(ip);
                SocketAddr::new(IpAddr::V6(ipv6), port)
            }
        } else {
            panic!("Unsupported address family {}", self.addr.ss_family);
        }
    }

    pub fn set_addr(&mut self, a: &SocketAddr) {
        match *a {
            SocketAddr::V4(v4) => {
                self.addr.ss_family = libc::AF_INET as sa_family_t;
                unsafe {
                    let mut p: *mut sockaddr_in = &mut self.addr as *mut _ as *mut _;
                    (*p).sin_port = v4.port();
                    (*p).sin_addr.s_addr = From::<Ipv4Addr>::from(*v4.ip());
                }
            }
            SocketAddr::V6(v6) => {
                self.addr.ss_family = libc::AF_INET6 as sa_family_t;
                unsafe {
                    let mut p: *mut sockaddr_in6 = &mut self.addr as *mut _ as *mut _;
                    (*p).sin6_port = v6.port();
                    (*p).sin6_flowinfo = 0u32;
                    (*p).sin6_addr.s6_addr = v6.ip().octets();
                    (*p).sin6_scope_id = 0u32;
                }
            }
        }
    }
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
            packet.meta.set_addr(dest);
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
            self.meta.addr()
        )
    }
}

#[allow(clippy::uninit_assumed_init)]
impl Default for Packet {
    fn default() -> Packet {
        Packet {
            data: unsafe { std::mem::MaybeUninit::uninit().assume_init() },
            meta: Meta::default(),
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
