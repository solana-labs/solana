//! The `packet` module defines data structures and methods to pull data from the network.
use crate::cuda_runtime::PinnedVec;
use crate::erasure::ErasureConfig;
use crate::recvmmsg::{recv_mmsg, NUM_RCVMMSGS};
use crate::recycler::{Recycler, Reset};
use crate::result::{Error, Result};
use bincode;
use byteorder::{ByteOrder, LittleEndian};
use serde::Serialize;
use solana_metrics::inc_new_counter_debug;
pub use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signable;
use solana_sdk::signature::Signature;
use std::borrow::Cow;
use std::cmp;
use std::fmt;
use std::io;
use std::io::Cursor;
use std::mem;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub type SharedBlob = Arc<RwLock<Blob>>;
pub type SharedBlobs = Vec<SharedBlob>;

pub const NUM_PACKETS: usize = 1024 * 8;
pub const BLOB_SIZE: usize = (2 * 1024 - 128); // wikipedia says there should be 20b for ipv4 headers
pub const BLOB_DATA_SIZE: usize = BLOB_SIZE - (BLOB_HEADER_SIZE * 2);
pub const BLOB_DATA_ALIGN: usize = 16; // safe for erasure input pointers, gf.c needs 16byte-aligned buffers
pub const NUM_BLOBS: usize = (NUM_PACKETS * PACKET_DATA_SIZE) / BLOB_SIZE;
pub const PACKETS_PER_BLOB: usize = 8; // reasonable estimate for payment packets per blob based on ~200b transaction size

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

#[derive(Debug, Clone)]
pub struct Packets {
    pub packets: PinnedVec<Packet>,

    recycler: Option<PacketsRecycler>,
}

impl Drop for Packets {
    fn drop(&mut self) {
        if let Some(ref recycler) = self.recycler {
            let old = mem::replace(&mut self.packets, PinnedVec::default());
            recycler.recycle(old)
        }
    }
}

impl Reset for Packets {
    fn reset(&mut self) {
        self.packets.resize(0, Packet::default());
    }
}

impl Reset for PinnedVec<Packet> {
    fn reset(&mut self) {
        self.resize(0, Packet::default());
    }
}

//auto derive doesn't support large arrays
impl Default for Packets {
    fn default() -> Packets {
        let packets = PinnedVec::with_capacity(NUM_RCVMMSGS);
        Packets {
            packets,
            recycler: None,
        }
    }
}

pub type PacketsRecycler = Recycler<PinnedVec<Packet>>;

impl Packets {
    pub fn new(packets: Vec<Packet>) -> Self {
        let packets = PinnedVec::from_vec(packets);
        Self {
            packets,
            recycler: None,
        }
    }

    pub fn new_with_recycler(recycler: PacketsRecycler, size: usize, name: &'static str) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve_and_pin(size);
        Packets {
            packets,
            recycler: Some(recycler),
        }
    }

    pub fn set_addr(&mut self, addr: &SocketAddr) {
        for m in self.packets.iter_mut() {
            m.meta.set_addr(&addr);
        }
    }
}

#[repr(align(16))] // 16 === BLOB_DATA_ALIGN
pub struct BlobData {
    pub data: [u8; BLOB_SIZE],
}

impl Clone for BlobData {
    fn clone(&self) -> Self {
        BlobData { data: self.data }
    }
}

impl Default for BlobData {
    fn default() -> Self {
        BlobData {
            data: [0u8; BLOB_SIZE],
        }
    }
}

impl PartialEq for BlobData {
    fn eq(&self, other: &BlobData) -> bool {
        let self_data: &[u8] = self.data.as_ref();
        let other_data: &[u8] = other.data.as_ref();
        self_data == other_data
    }
}

// this code hides _data, maps it to _data.data
impl Deref for Blob {
    type Target = BlobData;

    fn deref(&self) -> &Self::Target {
        &self._data
    }
}
impl DerefMut for Blob {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self._data
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct Blob {
    _data: BlobData, // hidden member, passed through by Deref
    pub meta: Meta,
}

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Blob {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.meta.addr()
        )
    }
}

#[derive(Debug)]
pub enum BlobError {
    /// the Blob's meta and data are not self-consistent
    BadState,
    /// Blob verification failed
    VerificationFailed,
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
        let start = Instant::now();
        let mut total_size = 0;
        loop {
            self.packets.resize(i + NUM_RCVMMSGS, Packet::default());
            match recv_mmsg(socket, &mut self.packets[i..]) {
                Err(_) if i > 0 => {
                    if start.elapsed().as_millis() > 1 {
                        break;
                    }
                }
                Err(e) => {
                    trace!("recv_from err {:?}", e);
                    return Err(Error::IO(e));
                }
                Ok((size, npkts)) => {
                    if i == 0 {
                        socket.set_nonblocking(true)?;
                    }
                    trace!("got {} packets", npkts);
                    i += npkts;
                    total_size += size;
                    // Try to batch into blob-sized buffers
                    // will cause less re-shuffling later on.
                    if start.elapsed().as_millis() > 1 || total_size >= (BLOB_DATA_SIZE - 512) {
                        break;
                    }
                }
            }
        }
        self.packets.truncate(i);
        inc_new_counter_debug!("packets-recv_count", i);
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

pub fn to_blob<T: Serialize>(resp: T, rsp_addr: SocketAddr) -> Result<Blob> {
    let mut b = Blob::default();
    let v = bincode::serialize(&resp)?;
    let len = v.len();
    assert!(len <= BLOB_SIZE);
    b.data[..len].copy_from_slice(&v);
    b.meta.size = len;
    b.meta.set_addr(&rsp_addr);
    Ok(b)
}

pub fn to_blobs<T: Serialize>(rsps: Vec<(T, SocketAddr)>) -> Result<Vec<Blob>> {
    let mut blobs = Vec::new();
    for (resp, rsp_addr) in rsps {
        blobs.push(to_blob(resp, rsp_addr)?);
    }
    Ok(blobs)
}

pub fn to_shared_blob<T: Serialize>(resp: T, rsp_addr: SocketAddr) -> Result<SharedBlob> {
    let blob = Arc::new(RwLock::new(to_blob(resp, rsp_addr)?));
    Ok(blob)
}

pub fn to_shared_blobs<T: Serialize>(rsps: Vec<(T, SocketAddr)>) -> Result<SharedBlobs> {
    let mut blobs = Vec::new();
    for (resp, rsp_addr) in rsps {
        blobs.push(to_shared_blob(resp, rsp_addr)?);
    }
    Ok(blobs)
}

macro_rules! range {
    ($prev:expr, $type:ident) => {
        $prev..$prev + size_of::<$type>()
    };
}

const SIGNATURE_RANGE: std::ops::Range<usize> = range!(0, Signature);
const FORWARDED_RANGE: std::ops::Range<usize> = range!(SIGNATURE_RANGE.end, bool);
const PARENT_RANGE: std::ops::Range<usize> = range!(FORWARDED_RANGE.end, u64);
const VERSION_RANGE: std::ops::Range<usize> = range!(PARENT_RANGE.end, u64);
const SLOT_RANGE: std::ops::Range<usize> = range!(VERSION_RANGE.end, u64);
const INDEX_RANGE: std::ops::Range<usize> = range!(SLOT_RANGE.end, u64);
const ID_RANGE: std::ops::Range<usize> = range!(INDEX_RANGE.end, Pubkey);
const FLAGS_RANGE: std::ops::Range<usize> = range!(ID_RANGE.end, u32);
const ERASURE_CONFIG_RANGE: std::ops::Range<usize> = range!(FLAGS_RANGE.end, ErasureConfig);
const SIZE_RANGE: std::ops::Range<usize> = range!(ERASURE_CONFIG_RANGE.end, u64);

macro_rules! align {
    ($x:expr, $align:expr) => {
        $x + ($align - 1) & !($align - 1)
    };
}

pub const BLOB_HEADER_SIZE: usize = align!(SIZE_RANGE.end, BLOB_DATA_ALIGN); // make sure data() is safe for erasure
pub const SIGNABLE_START: usize = PARENT_RANGE.start;

pub const BLOB_FLAG_IS_LAST_IN_SLOT: u32 = 0x2;

pub const BLOB_FLAG_IS_CODING: u32 = 0x1;

impl Blob {
    pub fn new(data: &[u8]) -> Self {
        let mut blob = Self::default();

        assert!(data.len() <= blob.data.len());

        let data_len = cmp::min(data.len(), blob.data.len());

        let bytes = &data[..data_len];
        blob.data[..data_len].copy_from_slice(bytes);
        blob.meta.size = data_len;
        blob
    }

    pub fn from_serializable<T: Serialize + ?Sized>(data: &T) -> Self {
        let mut blob = Self::default();
        let pos = {
            let mut out = Cursor::new(blob.data_mut());
            bincode::serialize_into(&mut out, data).expect("failed to serialize output");
            out.position() as usize
        };
        blob.set_size(pos);
        blob.set_erasure_config(&ErasureConfig::default());
        blob
    }

    pub fn parent(&self) -> u64 {
        LittleEndian::read_u64(&self.data[PARENT_RANGE])
    }
    pub fn set_parent(&mut self, ix: u64) {
        LittleEndian::write_u64(&mut self.data[PARENT_RANGE], ix);
    }
    pub fn version(&self) -> u64 {
        LittleEndian::read_u64(&self.data[VERSION_RANGE])
    }
    pub fn set_version(&mut self, version: u64) {
        LittleEndian::write_u64(&mut self.data[VERSION_RANGE], version);
    }
    pub fn slot(&self) -> u64 {
        LittleEndian::read_u64(&self.data[SLOT_RANGE])
    }
    pub fn set_slot(&mut self, ix: u64) {
        LittleEndian::write_u64(&mut self.data[SLOT_RANGE], ix);
    }
    pub fn index(&self) -> u64 {
        LittleEndian::read_u64(&self.data[INDEX_RANGE])
    }
    pub fn set_index(&mut self, ix: u64) {
        LittleEndian::write_u64(&mut self.data[INDEX_RANGE], ix);
    }

    pub fn set_erasure_config(&mut self, config: &ErasureConfig) {
        self.data[ERASURE_CONFIG_RANGE].copy_from_slice(&bincode::serialize(config).unwrap())
    }

    pub fn erasure_config(&self) -> ErasureConfig {
        bincode::deserialize(&self.data[ERASURE_CONFIG_RANGE]).unwrap_or_default()
    }

    pub fn seed(&self) -> [u8; 32] {
        let mut seed = [0; 32];
        let seed_len = seed.len();
        let signature_bytes = self.get_signature_bytes();
        seed[0..seed_len].copy_from_slice(&signature_bytes[(signature_bytes.len() - seed_len)..]);
        seed
    }

    /// sender id, we use this for identifying if its a blob from the leader that we should
    /// retransmit.  eventually blobs should have a signature that we can use for spam filtering
    pub fn id(&self) -> Pubkey {
        Pubkey::new(&self.data[ID_RANGE])
    }

    pub fn set_id(&mut self, id: &Pubkey) {
        self.data[ID_RANGE].copy_from_slice(id.as_ref())
    }

    /// Used to determine whether or not this blob should be forwarded in retransmit
    /// A bool is used here instead of a flag because this item is not intended to be signed when
    /// blob signatures are introduced
    pub fn should_forward(&self) -> bool {
        self.data[FORWARDED_RANGE][0] & 0x1 == 0
    }

    /// Mark this blob's forwarded status
    pub fn set_forwarded(&mut self, forward: bool) {
        self.data[FORWARDED_RANGE][0] = u8::from(forward)
    }

    pub fn flags(&self) -> u32 {
        LittleEndian::read_u32(&self.data[FLAGS_RANGE])
    }
    pub fn set_flags(&mut self, ix: u32) {
        LittleEndian::write_u32(&mut self.data[FLAGS_RANGE], ix);
    }

    pub fn is_coding(&self) -> bool {
        (self.flags() & BLOB_FLAG_IS_CODING) != 0
    }

    pub fn set_coding(&mut self) {
        let flags = self.flags();
        self.set_flags(flags | BLOB_FLAG_IS_CODING);
    }

    pub fn set_is_last_in_slot(&mut self) {
        let flags = self.flags();
        self.set_flags(flags | BLOB_FLAG_IS_LAST_IN_SLOT);
    }

    pub fn is_last_in_slot(&self) -> bool {
        (self.flags() & BLOB_FLAG_IS_LAST_IN_SLOT) != 0
    }

    pub fn data_size(&self) -> u64 {
        cmp::min(
            LittleEndian::read_u64(&self.data[SIZE_RANGE]),
            BLOB_SIZE as u64,
        )
    }

    pub fn set_data_size(&mut self, size: u64) {
        LittleEndian::write_u64(&mut self.data[SIZE_RANGE], size);
    }

    pub fn data(&self) -> &[u8] {
        &self.data[BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + BLOB_DATA_SIZE]
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data[BLOB_HEADER_SIZE..BLOB_HEADER_SIZE + BLOB_DATA_SIZE]
    }
    pub fn size(&self) -> usize {
        let size = self.data_size() as usize;

        if size > BLOB_HEADER_SIZE && size == self.meta.size {
            size - BLOB_HEADER_SIZE
        } else {
            0
        }
    }

    pub fn set_size(&mut self, size: usize) {
        let new_size = size + BLOB_HEADER_SIZE;
        self.meta.size = new_size;
        self.set_data_size(new_size as u64);
    }

    pub fn get_signature_bytes(&self) -> &[u8] {
        &self.data[SIGNATURE_RANGE]
    }

    pub fn recv_blob(socket: &UdpSocket, r: &SharedBlob) -> io::Result<()> {
        let mut p = r.write().unwrap();
        trace!("receiving on {}", socket.local_addr().unwrap());

        let (nrecv, from) = socket.recv_from(&mut p.data)?;
        p.meta.size = nrecv;
        p.meta.set_addr(&from);
        trace!("got {} bytes from {}", nrecv, from);
        Ok(())
    }

    pub fn recv_from(socket: &UdpSocket) -> Result<SharedBlobs> {
        let mut v = Vec::new();
        //DOCUMENTED SIDE-EFFECT
        //Performance out of the IO without poll
        //  * block on the socket until it's readable
        //  * set the socket to non blocking
        //  * read until it fails
        //  * set it back to blocking before returning
        socket.set_nonblocking(false)?;
        for i in 0..NUM_BLOBS {
            let r = SharedBlob::default();

            match Blob::recv_blob(socket, &r) {
                Err(_) if i > 0 => {
                    trace!("got {:?} messages on {}", i, socket.local_addr().unwrap());
                    break;
                }
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock && e.kind() != io::ErrorKind::TimedOut
                    {
                        info!("recv_from err {:?}", e);
                    }
                    return Err(Error::IO(e));
                }
                Ok(()) => {
                    if i == 0 {
                        socket.set_nonblocking(true)?;
                    }
                }
            }
            v.push(r);
        }
        Ok(v)
    }
    pub fn send_to(socket: &UdpSocket, v: SharedBlobs) -> Result<()> {
        for r in v {
            {
                let p = r.read().unwrap();
                let a = p.meta.addr();
                if let Err(e) = socket.send_to(&p.data[..p.meta.size], &a) {
                    warn!(
                        "error sending {} byte packet to {:?}: {:?}",
                        p.meta.size, a, e
                    );
                    Err(e)?;
                }
            }
        }
        Ok(())
    }
}

impl Signable for Blob {
    fn pubkey(&self) -> Pubkey {
        self.id()
    }

    fn signable_data(&self) -> Cow<[u8]> {
        let end = cmp::max(SIGNABLE_START, self.data_size() as usize);
        Cow::Borrowed(&self.data[SIGNABLE_START..end])
    }

    fn get_signature(&self) -> Signature {
        Signature::new(self.get_signature_bytes())
    }

    fn set_signature(&mut self, signature: Signature) {
        self.data[SIGNATURE_RANGE].copy_from_slice(signature.as_ref())
    }
}

pub fn index_blobs(blobs: &[SharedBlob], id: &Pubkey, mut blob_index: u64, slot: u64, parent: u64) {
    // enumerate all the blobs, those are the indices
    for blob in blobs.iter() {
        let mut blob = blob.write().unwrap();
        blob.set_index(blob_index);
        blob.set_slot(slot);
        blob.set_parent(parent);
        blob.set_id(id);
        blob_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::io;
    use std::io::Write;
    use std::net::{SocketAddr, UdpSocket};

    #[test]
    fn test_packets_set_addr() {
        // test that the address is actually being updated
        let send_addr = socketaddr!([127, 0, 0, 1], 123);
        let packets = vec![Packet::default()];
        let mut msgs = Packets::new(packets);
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

        for m in &p.packets {
            assert_eq!(m.meta.size, PACKET_DATA_SIZE);
            assert_eq!(m.meta.addr(), saddr);
        }
    }

    #[test]
    fn test_to_packets() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = system_transaction::create_user_account(&keypair, &keypair.pubkey(), 1, hash);
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
    pub fn blob_send_recv() {
        trace!("start");
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let p = SharedBlob::default();
        p.write().unwrap().meta.set_addr(&addr);
        p.write().unwrap().meta.size = 1024;
        let v = vec![p];
        Blob::send_to(&sender, v).unwrap();
        trace!("send_to");
        let rv = Blob::recv_from(&reader).unwrap();
        trace!("recv_from");
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().meta.size, 1024);
    }

    #[cfg(all(feature = "ipv6", test))]
    #[test]
    pub fn blob_ipv6_send_recv() {
        let reader = UdpSocket::bind("[::1]:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("[::1]:0").expect("bind");
        let p = SharedBlob::default();
        p.as_mut().unwrap().meta.set_addr(&addr);
        p.as_mut().unwrap().meta.size = 1024;
        let mut v = VecDeque::default();
        v.push_back(p);
        Blob::send_to(&r, &sender, &mut v).unwrap();
        let mut rv = Blob::recv_from(&reader).unwrap();
        let rp = rv.pop_front().unwrap();
        assert_eq!(rp.as_mut().meta.size, 1024);
    }

    #[test]
    pub fn debug_trait() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
        write!(io::sink(), "{:?}", Blob::default()).unwrap();
    }
    #[test]
    pub fn blob_test() {
        let mut b = Blob::default();
        b.set_index(<u64>::max_value());
        assert_eq!(b.index(), <u64>::max_value());
        b.data_mut()[0] = 1;
        assert_eq!(b.data()[0], 1);
        assert_eq!(b.index(), <u64>::max_value());
        assert_eq!(b.meta, Meta::default());
    }
    #[test]
    fn test_blob_forward() {
        let mut b = Blob::default();
        assert!(b.should_forward());
        b.set_forwarded(true);
        assert!(!b.should_forward());
    }

    #[test]
    fn test_blob_erasure_config() {
        let mut b = Blob::default();
        let config = ErasureConfig::new(32, 16);
        b.set_erasure_config(&config);

        assert_eq!(config, b.erasure_config());
    }

    #[test]
    fn test_blob_data_align() {
        assert_eq!(std::mem::align_of::<BlobData>(), BLOB_DATA_ALIGN);
    }

    #[test]
    fn test_packet_partial_eq() {
        let mut p1 = Packet::default();
        let mut p2 = Packet::default();

        p1.meta.size = 1;
        p1.data[0] = 0;

        p2.meta.size = 1;
        p2.data[0] = 0;

        assert!(p1 == p2);

        p2.data[0] = 4;
        assert!(p1 != p2);
    }

    #[test]
    fn test_blob_partial_eq() {
        let p1 = Blob::default();
        let mut p2 = Blob::default();

        assert!(p1 == p2);
        p2.data[1] = 4;
        assert!(p1 != p2);
    }

    #[test]
    fn test_sign_blob() {
        let mut b = Blob::default();
        let k = Keypair::new();
        let p = k.pubkey();
        b.set_id(&p);
        b.sign(&k);
        assert!(b.verify());

        // Set a bigger chunk of data to sign
        b.set_size(80);
        b.sign(&k);
        assert!(b.verify());
    }

    #[test]
    fn test_packets_reset() {
        let mut packets = Packets::default();
        packets.packets.resize(10, Packet::default());
        assert_eq!(packets.packets.len(), 10);
        packets.reset();
        assert_eq!(packets.packets.len(), 0);
    }

    #[test]
    fn test_version() {
        let mut b = Blob::default();
        assert_eq!(b.version(), 0);
        b.set_version(1);
        assert_eq!(b.version(), 1);
    }
}
