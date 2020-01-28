use crate::clock::Slot;
use bincode::Result;
use serde::Serialize;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    {fmt, io},
};

/// Maximum over-the-wire size of a Transaction
///   1280 is IPv6 minimum MTU
///   40 bytes is the size of the IPv6 header
///   8 bytes is the size of the fragment header
pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub forward: bool,
    pub repair: bool,
    pub discard: bool,
    pub addr: [u16; 8],
    pub port: u16,
    pub v6: bool,
    pub seed: [u8; 32],
    pub slot: Slot,
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

    pub fn from_data<T: Serialize>(dest: &SocketAddr, data: T) -> Self {
        let mut me = Packet::default();
        if let Err(e) = Self::populate_packet(&mut me, Some(dest), &data) {
            logger::error!("Couldn't write to packet {:?}. Data skipped.", e);
        }
        me
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
        self.meta == other.meta && self_data[..self.meta.size] == other_data[..other.meta.size]
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
