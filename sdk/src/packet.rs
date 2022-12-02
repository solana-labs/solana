//! The definition of a Solana network packet.

use {
    bincode::{Options, Result},
    bitflags::bitflags,
    serde::Serialize,
    std::{
        fmt, io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        slice::SliceIndex,
    },
};

#[cfg(test)]
static_assertions::const_assert_eq!(PACKET_DATA_SIZE, 1232);
/// Maximum over-the-wire size of a packet
///   1280 is IPv6 minimum MTU
///   40 bytes is the size of the IPv6 header
///   8 bytes is the size of the fragment header
pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;
#[cfg(test)]
static_assertions::const_assert_eq!(TRANSACTION_DATA_SIZE, 1232);
/// Maximum over-the-wire size of a transaction, currently two packets
pub const TRANSACTION_DATA_SIZE: usize = PACKET_DATA_SIZE;

bitflags! {
    #[repr(C)]
    pub struct PacketFlags: u8 {
        const DISCARD        = 0b0000_0001;
        const FORWARDED      = 0b0000_0010;
        const REPAIR         = 0b0000_0100;
        const SIMPLE_VOTE_TX = 0b0000_1000;
        const TRACER_PACKET  = 0b0001_0000;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub addr: IpAddr,
    pub port: u16,
    pub flags: PacketFlags,
    pub sender_stake: u64,
}

pub trait BasePacket: Default + Clone + Sync + Send + Eq + fmt::Debug {
    const DATA_SIZE: usize;
    fn populate_packet<T: Serialize>(&mut self, dest: Option<&SocketAddr>, data: &T) -> Result<()>;

    fn meta(&self) -> &Meta;
    fn meta_mut(&mut self) -> &mut Meta;
    fn buffer(&self) -> &[u8];

    /// Returns a mutable reference to the entirety of the underlying buffer to
    /// write into. The caller is responsible for updating Packet.meta.size
    /// after writing to the buffer.
    fn buffer_mut(&mut self) -> &mut [u8];

    /// Returns an immutable reference to the underlying buffer up to
    /// packet.meta.size. The rest of the buffer is not valid to read from.
    /// packet.data(..) returns packet.buffer.get(..packet.meta.size).
    /// Returns None if the index is invalid or if the packet is already marked
    /// as discard.
    #[inline]
    fn data<I>(&self, index: I) -> Option<&<I as SliceIndex<[u8]>>::Output>
    where
        I: SliceIndex<[u8]>,
    {
        // If the packet is marked as discard, it is either invalid or
        // otherwise should be ignored, and so the payload should not be read
        // from.
        if self.meta().discard() {
            None
        } else {
            self.buffer().get(..self.meta().size)?.get(index)
        }
    }

    fn from_data<T: Serialize>(dest: Option<&SocketAddr>, data: T) -> Result<Self> {
        let mut packet = Self::default();
        Self::populate_packet(&mut packet, dest, &data)?;
        Ok(packet)
    }

    fn deserialize_slice<T, I>(&self, index: I) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        I: SliceIndex<[u8], Output = [u8]>,
    {
        let bytes = self.data(index).ok_or(bincode::ErrorKind::SizeLimit)?;
        bincode::options()
            .with_limit(Self::DATA_SIZE as u64)
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .deserialize(bytes)
    }
}

macro_rules! impl_packet {
    ($P:ident, $S:ident) => {
        #[derive(Clone, Eq)]
        #[repr(C)]
        pub struct $P {
            // Bytes past Packet.meta.size are not valid to read from.
            // Use Packet.data(index) to read from the buffer.
            buffer: [u8; $S],
            meta: Meta,
        }

        impl $P {
            pub fn new(buffer: [u8; $S], meta: Meta) -> Self {
                Self { buffer, meta }
            }
        }

        impl BasePacket for $P {
            const DATA_SIZE: usize = $S;

            fn populate_packet<T: Serialize>(
                &mut self,
                dest: Option<&SocketAddr>,
                data: &T,
            ) -> Result<()> {
                debug_assert!(!self.meta.discard());
                let mut wr = io::Cursor::new(self.buffer_mut());
                bincode::serialize_into(&mut wr, data)?;
                self.meta.size = wr.position() as usize;
                if let Some(dest) = dest {
                    self.meta.set_socket_addr(dest);
                }
                Ok(())
            }

            fn meta(&self) -> &Meta {
                &self.meta
            }

            fn meta_mut(&mut self) -> &mut Meta {
                &mut self.meta
            }

            fn buffer(&self) -> &[u8] {
                &self.buffer
            }

            fn buffer_mut(&mut self) -> &mut [u8] {
                debug_assert!(!self.meta.discard());
                &mut self.buffer[..]
            }
        }

        impl fmt::Debug for $P {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "Packet {{ size: {:?}, addr: {:?} }}",
                    self.meta.size,
                    self.meta.socket_addr()
                )
            }
        }

        #[allow(clippy::uninit_assumed_init)]
        impl Default for $P {
            fn default() -> Self {
                let buffer = std::mem::MaybeUninit::<[u8; Self::DATA_SIZE]>::uninit();
                Self {
                    buffer: unsafe { buffer.assume_init() },
                    meta: Meta::default(),
                }
            }
        }

        impl PartialEq for $P {
            fn eq(&self, other: &Self) -> bool {
                self.meta == other.meta && self.data(..) == other.data(..)
            }
        }
    };
}
impl_packet!(Packet, PACKET_DATA_SIZE);
impl_packet!(TransactionPacket, TRANSACTION_DATA_SIZE);

impl Meta {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr, self.port)
    }

    pub fn set_socket_addr(&mut self, socket_addr: &SocketAddr) {
        self.addr = socket_addr.ip();
        self.port = socket_addr.port();
    }

    #[inline]
    pub fn discard(&self) -> bool {
        self.flags.contains(PacketFlags::DISCARD)
    }

    #[inline]
    pub fn set_discard(&mut self, discard: bool) {
        self.flags.set(PacketFlags::DISCARD, discard);
    }

    #[inline]
    pub fn set_tracer(&mut self, is_tracer: bool) {
        self.flags.set(PacketFlags::TRACER_PACKET, is_tracer);
    }

    #[inline]
    pub fn set_simple_vote(&mut self, is_simple_vote: bool) {
        self.flags.set(PacketFlags::SIMPLE_VOTE_TX, is_simple_vote);
    }

    #[inline]
    pub fn forwarded(&self) -> bool {
        self.flags.contains(PacketFlags::FORWARDED)
    }

    #[inline]
    pub fn repair(&self) -> bool {
        self.flags.contains(PacketFlags::REPAIR)
    }

    #[inline]
    pub fn is_simple_vote_tx(&self) -> bool {
        self.flags.contains(PacketFlags::SIMPLE_VOTE_TX)
    }

    #[inline]
    pub fn is_tracer_packet(&self) -> bool {
        self.flags.contains(PacketFlags::TRACER_PACKET)
    }
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            size: 0,
            addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
            flags: PacketFlags::empty(),
            sender_stake: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_slice() {
        let p = Packet::from_data(None, u32::MAX).unwrap();
        assert_eq!(p.deserialize_slice(..).ok(), Some(u32::MAX));
        assert_eq!(p.deserialize_slice(0..4).ok(), Some(u32::MAX));
        assert_eq!(
            p.deserialize_slice::<u16, _>(0..4)
                .map_err(|e| e.to_string()),
            Err("Slice had bytes remaining after deserialization".to_string()),
        );
        assert_eq!(
            p.deserialize_slice::<u32, _>(0..0)
                .map_err(|e| e.to_string()),
            Err("io error: unexpected end of file".to_string()),
        );
        assert_eq!(
            p.deserialize_slice::<u32, _>(0..1)
                .map_err(|e| e.to_string()),
            Err("io error: unexpected end of file".to_string()),
        );
        assert_eq!(
            p.deserialize_slice::<u32, _>(0..5)
                .map_err(|e| e.to_string()),
            Err("the size limit has been reached".to_string()),
        );
        #[allow(clippy::reversed_empty_ranges)]
        let reversed_empty_range = 4..0;
        assert_eq!(
            p.deserialize_slice::<u32, _>(reversed_empty_range)
                .map_err(|e| e.to_string()),
            Err("the size limit has been reached".to_string()),
        );
        assert_eq!(
            p.deserialize_slice::<u32, _>(4..5)
                .map_err(|e| e.to_string()),
            Err("the size limit has been reached".to_string()),
        );
    }
}
