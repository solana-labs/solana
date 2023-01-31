pub use crate::legacy_contact_info::LegacyContactInfo;
use {
    crate::crds_value::MAX_WALLCLOCK,
    matches::debug_assert_matches,
    serde::{Deserialize, Deserializer, Serialize},
    solana_sdk::{
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        serde_varint, short_vec,
    },
    static_assertions::const_assert_eq,
    std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        time::{SystemTime, UNIX_EPOCH},
    },
    thiserror::Error,
};

const SOCKET_TAG_GOSSIP: u8 = 0;
const SOCKET_TAG_REPAIR: u8 = 1;
const SOCKET_TAG_RPC: u8 = 2;
const SOCKET_TAG_RPC_PUBSUB: u8 = 3;
const SOCKET_TAG_SERVE_REPAIR: u8 = 4;
const SOCKET_TAG_TPU: u8 = 5;
const SOCKET_TAG_TPU_FORWARDS: u8 = 6;
const SOCKET_TAG_TPU_FORWARDS_QUIC: u8 = 7;
const SOCKET_TAG_TPU_QUIC: u8 = 8;
const SOCKET_TAG_TPU_VOTE: u8 = 9;
const SOCKET_TAG_TVU: u8 = 10;
const SOCKET_TAG_TVU_FORWARDS: u8 = 11;
const_assert_eq!(SOCKET_CACHE_SIZE, 12);
const SOCKET_CACHE_SIZE: usize = SOCKET_TAG_TVU_FORWARDS as usize + 1usize;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Duplicate IP address: {0}")]
    DuplicateIpAddr(IpAddr),
    #[error("Duplicate socket: {0}")]
    DuplicateSocket(/*key:*/ u8),
    #[error("Invalid IP address index: {index}, num addrs: {num_addrs}")]
    InvalidIpAddrIndex { index: u8, num_addrs: usize },
    #[error("Invalid port: {0}")]
    InvalidPort(/*port:*/ u16),
    #[error("IP addresses saturated")]
    IpAddrsSaturated,
    #[error("Multicast IP address: {0}")]
    MulticastIpAddr(IpAddr),
    #[error("Port offsets overflow")]
    PortOffsetsOverflow,
    #[error("Socket not found: {0}")]
    SocketNotFound(/*key:*/ u8),
    #[error("Unspecified IP address: {0}")]
    UnspecifiedIpAddr(IpAddr),
    #[error("Unused IP address: {0}")]
    UnusedIpAddr(IpAddr),
}

#[derive(Clone, Debug, Eq, PartialEq, AbiExample, Serialize)]
pub struct ContactInfo {
    pubkey: Pubkey,
    #[serde(with = "serde_varint")]
    wallclock: u64,
    // When the node instance was first created.
    // Identifies duplicate running instances.
    outset: u64,
    shred_version: u16,
    version: solana_version::Version,
    // All IP addresses are unique and referenced at least once in sockets.
    #[serde(with = "short_vec")]
    addrs: Vec<IpAddr>,
    // All sockets have a unique key and a valid IP address index.
    #[serde(with = "short_vec")]
    sockets: Vec<SocketEntry>,
    #[serde(skip_serializing)]
    cache: [SocketAddr; SOCKET_CACHE_SIZE],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, AbiExample, Deserialize, Serialize)]
struct SocketEntry {
    key: u8,   // Protocol identifier, e.g. tvu, tpu, etc
    index: u8, // IpAddr index in the accompanying addrs vector.
    #[serde(with = "serde_varint")]
    offset: u16, // Port offset with respect to the previous entry.
}

// As part of deserialization, self.addrs and self.sockets should be cross
// verified and self.cache needs to be populated. This type serves as a
// workaround since serde does not have an initializer.
// https://github.com/serde-rs/serde/issues/642
#[derive(Deserialize)]
struct ContactInfoLite {
    pubkey: Pubkey,
    #[serde(with = "serde_varint")]
    wallclock: u64,
    outset: u64,
    shred_version: u16,
    version: solana_version::Version,
    #[serde(with = "short_vec")]
    addrs: Vec<IpAddr>,
    #[serde(with = "short_vec")]
    sockets: Vec<SocketEntry>,
}

macro_rules! get_socket {
    ($name:ident, $key:ident) => {
        pub fn $name(&self) -> Result<SocketAddr, Error> {
            let socket = self.cache[usize::from($key)];
            sanitize_socket(&socket)?;
            Ok(socket)
        }
    };
}

impl ContactInfo {
    pub fn new(pubkey: Pubkey, wallclock: u64, shred_version: u16) -> Self {
        Self {
            pubkey,
            wallclock,
            outset: {
                let now = SystemTime::now();
                let elapsed = now.duration_since(UNIX_EPOCH).unwrap();
                u64::try_from(elapsed.as_micros()).unwrap()
            },
            shred_version,
            version: solana_version::Version::default(),
            addrs: Vec::<IpAddr>::default(),
            sockets: Vec::<SocketEntry>::default(),
            cache: [socket_addr_unspecified(); SOCKET_CACHE_SIZE],
        }
    }

    #[inline]
    pub(crate) fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }

    #[inline]
    pub(crate) fn wallclock(&self) -> u64 {
        self.wallclock
    }

    get_socket!(gossip, SOCKET_TAG_GOSSIP);
    get_socket!(repair, SOCKET_TAG_REPAIR);
    get_socket!(rpc, SOCKET_TAG_RPC);
    get_socket!(rpc_pubsub, SOCKET_TAG_RPC_PUBSUB);
    get_socket!(serve_repair, SOCKET_TAG_SERVE_REPAIR);
    get_socket!(tpu, SOCKET_TAG_TPU);
    get_socket!(tpu_forwards, SOCKET_TAG_TPU_FORWARDS);
    get_socket!(tpu_forwards_quic, SOCKET_TAG_TPU_FORWARDS_QUIC);
    get_socket!(tpu_quic, SOCKET_TAG_TPU_QUIC);
    get_socket!(tpu_vote, SOCKET_TAG_TPU_VOTE);
    get_socket!(tvu, SOCKET_TAG_TVU);
    get_socket!(tvu_forwards, SOCKET_TAG_TVU_FORWARDS);

    #[cfg(test)]
    fn get_socket(&self, key: u8) -> Result<SocketAddr, Error> {
        let mut port = 0u16;
        for entry in &self.sockets {
            port += entry.offset;
            if entry.key == key {
                let addr =
                    self.addrs
                        .get(usize::from(entry.index))
                        .ok_or(Error::InvalidIpAddrIndex {
                            index: entry.index,
                            num_addrs: self.addrs.len(),
                        })?;
                let socket = SocketAddr::new(*addr, port);
                sanitize_socket(&socket)?;
                return Ok(socket);
            }
        }
        Err(Error::SocketNotFound(key))
    }

    // Adds given IP address to self.addrs returning respective index.
    fn push_addr(&mut self, addr: IpAddr) -> Result<u8, Error> {
        match self.addrs.iter().position(|k| k == &addr) {
            Some(index) => u8::try_from(index).map_err(|_| Error::IpAddrsSaturated),
            None => {
                let index = u8::try_from(self.addrs.len()).map_err(|_| Error::IpAddrsSaturated)?;
                self.addrs.push(addr);
                Ok(index)
            }
        }
    }

    pub fn set_socket(&mut self, key: u8, socket: SocketAddr) -> Result<(), Error> {
        sanitize_socket(&socket)?;
        // Remove the old entry associated with this key (if any).
        self.remove_socket(key);
        // Find the index at which the new socket entry would be inserted into
        // self.sockets, and the respective port offset.
        let mut offset = socket.port();
        let index = self.sockets.iter().position(|entry| {
            offset = match offset.checked_sub(entry.offset) {
                None => return true,
                Some(offset) => offset,
            };
            false
        });
        let entry = SocketEntry {
            key,
            index: self.push_addr(socket.ip())?,
            offset,
        };
        // Insert the new entry into self.sockets.
        // Adjust the port offset of the next entry (if any).
        match index {
            None => self.sockets.push(entry),
            Some(index) => {
                self.sockets[index].offset -= entry.offset;
                self.sockets.insert(index, entry);
            }
        }
        if let Some(entry) = self.cache.get_mut(usize::from(key)) {
            *entry = socket;
        }
        debug_assert_matches!(sanitize_entries(&self.addrs, &self.sockets), Ok(()));
        Ok(())
    }

    // Removes the socket associated with the specified key.
    fn remove_socket(&mut self, key: u8) {
        if let Some(index) = self.sockets.iter().position(|entry| entry.key == key) {
            let entry = self.sockets.remove(index);
            if let Some(next_entry) = self.sockets.get_mut(index) {
                next_entry.offset += entry.offset;
            }
            self.maybe_remove_addr(entry.index);
            if let Some(entry) = self.cache.get_mut(usize::from(key)) {
                *entry = socket_addr_unspecified();
            }
        }
    }

    // Removes the IP address at the given index if
    // no socket entry refrences that index.
    fn maybe_remove_addr(&mut self, index: u8) {
        if !self.sockets.iter().any(|entry| entry.index == index) {
            self.addrs.remove(usize::from(index));
            for entry in &mut self.sockets {
                if entry.index > index {
                    entry.index -= 1;
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for ContactInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let node = ContactInfoLite::deserialize(deserializer)?;
        ContactInfo::try_from(node).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<ContactInfoLite> for ContactInfo {
    type Error = Error;

    fn try_from(node: ContactInfoLite) -> Result<Self, Self::Error> {
        let ContactInfoLite {
            pubkey,
            wallclock,
            outset,
            shred_version,
            version,
            addrs,
            sockets,
        } = node;
        sanitize_entries(&addrs, &sockets)?;
        let mut node = ContactInfo {
            pubkey,
            wallclock,
            outset,
            shred_version,
            version,
            addrs,
            sockets,
            cache: [socket_addr_unspecified(); SOCKET_CACHE_SIZE],
        };
        // Populate node.cache.
        let mut port = 0u16;
        for &SocketEntry { key, index, offset } in &node.sockets {
            port += offset;
            let entry = match node.cache.get_mut(usize::from(key)) {
                None => continue,
                Some(entry) => entry,
            };
            let addr = match node.addrs.get(usize::from(index)) {
                None => continue,
                Some(&addr) => addr,
            };
            let socket = SocketAddr::new(addr, port);
            if sanitize_socket(&socket).is_ok() {
                *entry = socket;
            }
        }
        Ok(node)
    }
}

impl Sanitize for ContactInfo {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}

// Workaround until feature(const_socketaddr) is stable.
fn socket_addr_unspecified() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), /*port:*/ 0u16)
}

fn sanitize_socket(socket: &SocketAddr) -> Result<(), Error> {
    if socket.port() == 0u16 {
        return Err(Error::InvalidPort(socket.port()));
    }
    let addr = socket.ip();
    if addr.is_unspecified() {
        return Err(Error::UnspecifiedIpAddr(addr));
    }
    if addr.is_multicast() {
        return Err(Error::MulticastIpAddr(addr));
    }
    Ok(())
}

// Sanitizes deserialized IpAddr and socket entries.
fn sanitize_entries(addrs: &[IpAddr], sockets: &[SocketEntry]) -> Result<(), Error> {
    // Verify that all IP addresses are unique.
    {
        let mut seen = HashSet::with_capacity(addrs.len());
        for addr in addrs {
            if !seen.insert(addr) {
                return Err(Error::DuplicateIpAddr(*addr));
            }
        }
    }
    // Verify that all socket entries have unique key.
    {
        let mut mask = [0u64; 4]; // 256-bit bitmask.
        for &SocketEntry { key, .. } in sockets {
            let mask = &mut mask[usize::from(key / 64u8)];
            let bit = 1u64 << (key % 64u8);
            if (*mask & bit) != 0u64 {
                return Err(Error::DuplicateSocket(key));
            }
            *mask |= bit;
        }
    }
    // Verify that all socket entries reference a valid IP address, and
    // that all IP addresses are referenced in the sockets.
    {
        let num_addrs = addrs.len();
        let mut hits = vec![false; num_addrs];
        for &SocketEntry { index, .. } in sockets {
            *hits
                .get_mut(usize::from(index))
                .ok_or(Error::InvalidIpAddrIndex { index, num_addrs })? = true;
        }
        if let Some(index) = hits.into_iter().position(|hit| !hit) {
            return Err(Error::UnusedIpAddr(addrs[index]));
        }
    }
    // Verify that port offsets don't overflow.
    if sockets
        .iter()
        .fold(Some(0u16), |offset, entry| {
            offset?.checked_add(entry.offset)
        })
        .is_none()
    {
        return Err(Error::PortOffsetsOverflow);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand::{seq::SliceRandom, Rng},
        std::{
            collections::{HashMap, HashSet},
            iter::repeat_with,
            net::{Ipv4Addr, Ipv6Addr},
            ops::Range,
        },
    };

    fn new_rand_addr<R: Rng>(rng: &mut R) -> IpAddr {
        if rng.gen() {
            let addr = Ipv4Addr::new(rng.gen(), rng.gen(), rng.gen(), rng.gen());
            IpAddr::V4(addr)
        } else {
            let addr = Ipv6Addr::new(
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
                rng.gen(),
            );
            IpAddr::V6(addr)
        }
    }

    fn new_rand_port<R: Rng>(rng: &mut R) -> u16 {
        let port = rng.gen::<u16>();
        let bits = u16::BITS - port.leading_zeros();
        let shift = rng.gen_range(0u32, bits + 1u32);
        port.checked_shr(shift).unwrap_or_default()
    }

    #[test]
    fn test_sanitize_entries() {
        let mut rng = rand::thread_rng();
        let addrs: Vec<IpAddr> = repeat_with(|| new_rand_addr(&mut rng)).take(5).collect();
        let mut keys: Vec<u8> = (0u8..=u8::MAX).collect();
        keys.shuffle(&mut rng);
        // Duplicate IP addresses.
        {
            let addrs = [addrs[0], addrs[1], addrs[2], addrs[0], addrs[3]];
            assert_matches!(
                sanitize_entries(&addrs, /*sockets:*/ &[]),
                Err(Error::DuplicateIpAddr(_))
            );
        }
        // Duplicate socket keys.
        {
            let keys = [0u8, 1, 5, 1, 3];
            let (index, offset) = (0u8, 0u16);
            let sockets: Vec<_> = keys
                .iter()
                .map(|&key| SocketEntry { key, index, offset })
                .collect();
            assert_matches!(
                sanitize_entries(/*addrs:*/ &[], &sockets),
                Err(Error::DuplicateSocket(_))
            );
        }
        // Invalid IP address index.
        {
            let offset = 0u16;
            let sockets: Vec<_> = [1u8, 2, 1, 3, 5, 3]
                .into_iter()
                .zip(&keys)
                .map(|(index, &key)| SocketEntry { key, index, offset })
                .collect();
            assert_matches!(
                sanitize_entries(&addrs, &sockets),
                Err(Error::InvalidIpAddrIndex { .. })
            );
        }
        // Unused IP address.
        {
            let sockets: Vec<_> = (0..4u8)
                .map(|key| SocketEntry {
                    key,
                    index: key,
                    offset: 0u16,
                })
                .collect();
            assert_matches!(
                sanitize_entries(&addrs, &sockets),
                Err(Error::UnusedIpAddr(_))
            );
        }
        // Port offsets overflow.
        {
            let sockets: Vec<_> = keys
                .iter()
                .map(|&key| SocketEntry {
                    key,
                    index: rng.gen_range(0u8, addrs.len() as u8),
                    offset: rng.gen_range(0u16, u16::MAX / 64),
                })
                .collect();
            assert_matches!(
                sanitize_entries(&addrs, &sockets),
                Err(Error::PortOffsetsOverflow)
            );
        }
        {
            let sockets: Vec<_> = keys
                .iter()
                .map(|&key| SocketEntry {
                    key,
                    index: rng.gen_range(0u8, addrs.len() as u8),
                    offset: rng.gen_range(0u16, u16::MAX / 256),
                })
                .collect();
            assert_matches!(sanitize_entries(&addrs, &sockets), Ok(()));
        }
    }

    #[test]
    fn test_round_trip() {
        const KEYS: Range<u8> = 0u8..16u8;
        let mut rng = rand::thread_rng();
        let addrs: Vec<IpAddr> = repeat_with(|| new_rand_addr(&mut rng)).take(8).collect();
        let mut node = ContactInfo {
            pubkey: Pubkey::new_unique(),
            wallclock: rng.gen(),
            outset: rng.gen(),
            shred_version: rng.gen(),
            version: solana_version::Version::default(),
            addrs: Vec::default(),
            sockets: Vec::default(),
            cache: [socket_addr_unspecified(); SOCKET_CACHE_SIZE],
        };
        let mut sockets = HashMap::<u8, SocketAddr>::new();
        for _ in 0..1 << 14 {
            let addr = addrs.choose(&mut rng).unwrap();
            let socket = SocketAddr::new(*addr, new_rand_port(&mut rng));
            let key = rng.gen_range(KEYS.start, KEYS.end);
            if sanitize_socket(&socket).is_ok() {
                sockets.insert(key, socket);
                assert_matches!(node.set_socket(key, socket), Ok(()));
                assert_matches!(sanitize_entries(&node.addrs, &node.sockets), Ok(()));
            } else {
                assert_matches!(node.set_socket(key, socket), Err(_));
            }
            for key in KEYS.clone() {
                let socket = sockets.get(&key);
                assert_eq!(node.get_socket(key).ok().as_ref(), socket);
                if usize::from(key) < SOCKET_CACHE_SIZE {
                    assert_eq!(
                        &node.cache[usize::from(key)],
                        socket.unwrap_or(&socket_addr_unspecified())
                    )
                }
            }
            assert_eq!(node.gossip().ok().as_ref(), sockets.get(&SOCKET_TAG_GOSSIP));
            assert_eq!(node.repair().ok().as_ref(), sockets.get(&SOCKET_TAG_REPAIR));
            assert_eq!(node.rpc().ok().as_ref(), sockets.get(&SOCKET_TAG_RPC));
            assert_eq!(
                node.rpc_pubsub().ok().as_ref(),
                sockets.get(&SOCKET_TAG_RPC_PUBSUB)
            );
            assert_eq!(
                node.serve_repair().ok().as_ref(),
                sockets.get(&SOCKET_TAG_SERVE_REPAIR)
            );
            assert_eq!(node.tpu().ok().as_ref(), sockets.get(&SOCKET_TAG_TPU));
            assert_eq!(
                node.tpu_forwards().ok().as_ref(),
                sockets.get(&SOCKET_TAG_TPU_FORWARDS)
            );
            assert_eq!(
                node.tpu_forwards_quic().ok().as_ref(),
                sockets.get(&SOCKET_TAG_TPU_FORWARDS_QUIC)
            );
            assert_eq!(
                node.tpu_quic().ok().as_ref(),
                sockets.get(&SOCKET_TAG_TPU_QUIC)
            );
            assert_eq!(
                node.tpu_vote().ok().as_ref(),
                sockets.get(&SOCKET_TAG_TPU_VOTE)
            );
            assert_eq!(node.tvu().ok().as_ref(), sockets.get(&SOCKET_TAG_TVU));
            assert_eq!(
                node.tvu_forwards().ok().as_ref(),
                sockets.get(&SOCKET_TAG_TVU_FORWARDS)
            );
            // Assert that all IP addresses are unique.
            assert_eq!(
                node.addrs.len(),
                node.addrs
                    .iter()
                    .copied()
                    .collect::<HashSet<IpAddr>>()
                    .len()
            );
            // Assert that all sockets have unique key.
            assert_eq!(
                node.sockets.len(),
                node.sockets
                    .iter()
                    .map(|entry| entry.key)
                    .collect::<HashSet<u8>>()
                    .len()
            );
            // Assert that only mapped addresses are stored.
            assert_eq!(
                node.addrs.iter().copied().collect::<HashSet<_>>(),
                sockets.values().map(SocketAddr::ip).collect::<HashSet<_>>(),
            );
            // Assert that all sockets reference a valid IP address.
            assert!(node
                .sockets
                .iter()
                .map(|entry| node.addrs.get(usize::from(entry.index)))
                .all(|addr| addr.is_some()));
            // Assert that port offsets don't overflow.
            assert!(u16::try_from(
                node.sockets
                    .iter()
                    .map(|entry| u64::from(entry.offset))
                    .sum::<u64>()
            )
            .is_ok());
            // Assert that serde round trips.
            let bytes = bincode::serialize(&node).unwrap();
            let other: ContactInfo = bincode::deserialize(&bytes).unwrap();
            assert_eq!(node, other);
        }
    }
}
