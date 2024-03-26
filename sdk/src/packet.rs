//! The definition of a Solana network packet.

use {
    bincode::{Options, Result},
    bitflags::bitflags,
    serde::{Deserialize, Serialize},
    serde_with::{serde_as, Bytes},
    std::{
        fmt, io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        slice::SliceIndex,
    },
};

#[cfg(test)]
static_assertions::const_assert_eq!(PACKET_DATA_SIZE, 1232);
/// Maximum over-the-wire size of a Transaction
///   1280 is IPv6 minimum MTU
///   40 bytes is the size of the IPv6 header
///   8 bytes is the size of the fragment header
pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;

bitflags! {
    #[repr(C)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PacketFlags: u8 {
        const DISCARD        = 0b0000_0001;
        const FORWARDED      = 0b0000_0010;
        const REPAIR         = 0b0000_0100;
        const SIMPLE_VOTE_TX = 0b0000_1000;
        const TRACER_PACKET  = 0b0001_0000;
        /// to be set by bank.feature_set.is_active(round_compute_unit_price::id()) at the moment
        /// the packet is built.
        /// This field can be removed when the above feature gate is adopted by mainnet-beta.
        const ROUND_COMPUTE_UNIT_PRICE = 0b0010_0000;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, AbiExample)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub addr: IpAddr,
    pub port: u16,
    pub flags: PacketFlags,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for PacketFlags {
    fn example() -> Self {
        Self::empty()
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::IgnoreAsHelper for PacketFlags {}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::EvenAsOpaque for PacketFlags {
    const TYPE_NAME_MATCHER: &'static str = "::_::InternalBitFlags";
}

// serde_as is used as a work around because array isn't supported by serde
// (and serde_bytes).
//
// the root cause is of a historical special handling for [T; 0] in rust's
// `Default` and supposedly mirrored serde's `Serialize` (macro) impls,
// pre-dating stabilized const generics, meaning it'll take long time...:
//   https://github.com/rust-lang/rust/issues/61415
//   https://github.com/rust-lang/rust/issues/88744#issuecomment-1138678928
//
// Due to the nature of the root cause, the current situation is complicated.
// All in all, the serde_as solution is chosen for good perf and low maintenance
// need at the cost of another crate dependency..
//
// For details, please refer to the below various links...
//
// relevant merged/published pr for this serde_as functionality used here:
//   https://github.com/jonasbb/serde_with/pull/277
// open pr at serde_bytes:
//   https://github.com/serde-rs/bytes/pull/28
// open issue at serde:
//   https://github.com/serde-rs/serde/issues/1937
// closed pr at serde (due to the above mentioned [N; 0] issue):
//   https://github.com/serde-rs/serde/pull/1860
// ryoqun's dirty experiments:
//   https://github.com/ryoqun/serde-array-comparisons
#[serde_as]
#[derive(Clone, Eq, Serialize, Deserialize, AbiExample)]
#[repr(C)]
pub struct Packet {
    // Bytes past Packet.meta.size are not valid to read from.
    // Use Packet.data(index) to read from the buffer.
    #[serde_as(as = "Bytes")]
    buffer: [u8; PACKET_DATA_SIZE],
    meta: Meta,
}

impl Packet {
    pub fn new(buffer: [u8; PACKET_DATA_SIZE], meta: Meta) -> Self {
        Self { buffer, meta }
    }

    /// Returns an immutable reference to the underlying buffer up to
    /// packet.meta.size. The rest of the buffer is not valid to read from.
    /// packet.data(..) returns packet.buffer.get(..packet.meta.size).
    /// Returns None if the index is invalid or if the packet is already marked
    /// as discard.
    #[inline]
    pub fn data<I>(&self, index: I) -> Option<&<I as SliceIndex<[u8]>>::Output>
    where
        I: SliceIndex<[u8]>,
    {
        // If the packet is marked as discard, it is either invalid or
        // otherwise should be ignored, and so the payload should not be read
        // from.
        if self.meta.discard() {
            None
        } else {
            self.buffer.get(..self.meta.size)?.get(index)
        }
    }

    /// Returns a mutable reference to the entirety of the underlying buffer to
    /// write into. The caller is responsible for updating Packet.meta.size
    /// after writing to the buffer.
    #[inline]
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        debug_assert!(!self.meta.discard());
        &mut self.buffer[..]
    }

    #[inline]
    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    #[inline]
    pub fn meta_mut(&mut self) -> &mut Meta {
        &mut self.meta
    }

    pub fn from_data<T: Serialize>(dest: Option<&SocketAddr>, data: T) -> Result<Self> {
        let mut packet = Self::default();
        Self::populate_packet(&mut packet, dest, &data)?;
        Ok(packet)
    }

    pub fn populate_packet<T: Serialize>(
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

    pub fn deserialize_slice<T, I>(&self, index: I) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        I: SliceIndex<[u8], Output = [u8]>,
    {
        let bytes = self.data(index).ok_or(bincode::ErrorKind::SizeLimit)?;
        bincode::options()
            .with_limit(PACKET_DATA_SIZE as u64)
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .deserialize(bytes)
    }
}

impl fmt::Debug for Packet {
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
impl Default for Packet {
    fn default() -> Self {
        let buffer = std::mem::MaybeUninit::<[u8; PACKET_DATA_SIZE]>::uninit();
        Self {
            buffer: unsafe { buffer.assume_init() },
            meta: Meta::default(),
        }
    }
}

impl PartialEq for Packet {
    fn eq(&self, other: &Self) -> bool {
        self.meta() == other.meta() && self.data(..) == other.data(..)
    }
}

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
    pub fn set_round_compute_unit_price(&mut self, round_compute_unit_price: bool) {
        self.flags.set(
            PacketFlags::ROUND_COMPUTE_UNIT_PRICE,
            round_compute_unit_price,
        );
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

    #[inline]
    pub fn round_compute_unit_price(&self) -> bool {
        self.flags.contains(PacketFlags::ROUND_COMPUTE_UNIT_PRICE)
    }
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            size: 0,
            addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
            flags: PacketFlags::empty(),
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
