//! The `blob` module defines data structures and methods used in the data plane protocol
use crate::packet::{Meta, Packet, NUM_PACKETS};
use crate::result::{Error, Result};
use bincode;
use byteorder::{ByteOrder, LittleEndian};
use serde::Serialize;
use solana_sdk::hash::Hash;
pub use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use std::borrow::Borrow;
use std::cmp;
use std::fmt;
use std::io;
use std::io::Cursor;
use std::io::Write;
use std::mem::size_of;
use std::net::{SocketAddr, UdpSocket};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, RwLock};

pub type SharedBlob = Arc<RwLock<Blob>>;
pub type SharedBlobs = Vec<SharedBlob>;

pub const BLOB_SIZE: usize = (64 * 1024 - 128); // wikipedia says there should be 20b for ipv4 headers
pub const BLOB_DATA_SIZE: usize = BLOB_SIZE - (BLOB_HEADER_SIZE * 2);
pub const NUM_BLOBS: usize = (NUM_PACKETS * PACKET_DATA_SIZE) / BLOB_SIZE;
pub const BLOB_HEADER_SIZE: usize = SIZE_RANGE.end;
pub const BLOB_FLAG_IS_LAST_IN_SLOT: u32 = 0x2;
pub const BLOB_FLAG_IS_CODING: u32 = 0x1;

#[derive(Clone, Default, PartialEq)]
pub struct Blob {
    _data: BlobData, // hidden member, passed through by Deref
    pub meta: Meta,
}

pub struct BlobData {
    pub data: [u8; BLOB_SIZE],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataBlob {
    blob: Blob,
    pub header: DataHeader,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodingBlob {
    blob: Blob,
    pub header: CodingHeader,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DataHeader {
    pub parent: u64,
    pub slot: u64,
    pub index: u64,
    pub id: Pubkey,
    pub should_forward: bool,
    pub genesis: Hash,
    pub is_last_in_slot: bool,
    pub data_size: u64,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CodingHeader {
    pub slot: u64,
    pub index: u64,
    pub id: Pubkey,
    pub data_size: u64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BlobHeader {
    Data(DataHeader),
    Coding(CodingHeader),
}

#[derive(Debug)]
pub enum BlobError {
    /// the Blob's meta and data are not self-consistent
    BadState,
    /// Blob verification failed
    VerificationFailed,
}

macro_rules! range {
    ($prev:expr, $type:ident) => {
        $prev..$prev + size_of::<$type>()
    };
}

const PARENT_RANGE: std::ops::Range<usize> = range!(0, u64);
const SLOT_RANGE: std::ops::Range<usize> = range!(PARENT_RANGE.end, u64);
const INDEX_RANGE: std::ops::Range<usize> = range!(SLOT_RANGE.end, u64);
const ID_RANGE: std::ops::Range<usize> = range!(INDEX_RANGE.end, Pubkey);
const FORWARDED_RANGE: std::ops::Range<usize> = range!(ID_RANGE.end, bool);
const GENESIS_RANGE: std::ops::Range<usize> = range!(FORWARDED_RANGE.end, Hash);
const FLAGS_RANGE: std::ops::Range<usize> = range!(GENESIS_RANGE.end, u32);
const SIZE_RANGE: std::ops::Range<usize> = range!(FLAGS_RANGE.end, u64);

impl Blob {
    pub fn new(data: &[u8]) -> Self {
        let mut blob = Self::default();

        assert!(data.len() <= blob.data.len());

        let data_len = cmp::min(data.len(), blob.data.len());

        let bytes = &data[..data_len];
        blob.data[..data_len].copy_from_slice(bytes);
        blob.meta.size = blob.data_size() as usize;
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
        blob
    }

    pub fn parent(&self) -> u64 {
        LittleEndian::read_u64(&self.data[PARENT_RANGE])
    }
    pub fn set_parent(&mut self, ix: u64) {
        LittleEndian::write_u64(&mut self.data[PARENT_RANGE], ix);
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

    pub fn set_genesis_blockhash(&mut self, blockhash: &Hash) {
        self.data[GENESIS_RANGE].copy_from_slice(blockhash.as_ref())
    }

    pub fn genesis_blockhash(&self) -> Hash {
        Hash::new(&self.data[GENESIS_RANGE])
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
        LittleEndian::read_u64(&self.data[SIZE_RANGE])
    }

    pub fn set_data_size(&mut self, size: u64) {
        LittleEndian::write_u64(&mut self.data[SIZE_RANGE], size);
    }

    pub fn data(&self) -> &[u8] {
        &self.data[BLOB_HEADER_SIZE..]
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data[BLOB_HEADER_SIZE..]
    }

    pub fn header(&self) -> &[u8] {
        &self.data[..BLOB_HEADER_SIZE]
    }

    pub fn header_mut(&mut self) -> &mut [u8] {
        &mut self.data[..BLOB_HEADER_SIZE]
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

    pub fn store_packets<T: Borrow<Packet>>(&mut self, packets: &[T]) -> u64 {
        let size = self.size();
        let mut cursor = Cursor::new(&mut self.data_mut()[size..]);
        let mut written = 0;
        let mut last_index = 0;
        for packet in packets {
            if bincode::serialize_into(&mut cursor, &packet.borrow().meta).is_err() {
                break;
            }
            if cursor.write_all(&packet.borrow().data[..]).is_err() {
                break;
            }

            written = cursor.position() as usize;
            last_index += 1;
        }

        self.set_size(size + written);
        last_index
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
                    if e.kind() != io::ErrorKind::WouldBlock {
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

    pub fn read_header(&self) -> BlobHeader {
        BlobHeader::deserialize(self.header()).unwrap()
    }

    pub fn into_coding(self, header: CodingHeader) -> CodingBlob {
        CodingBlob { blob: self, header }
    }

    pub fn into_data(self, header: DataHeader) -> DataBlob {
        DataBlob { blob: self, header }
    }
}

impl BlobHeader {
    pub fn serialize(&self, buffer: &mut [u8]) {
        assert!(buffer.len() >= BLOB_HEADER_SIZE);

        match self {
            BlobHeader::Data(ref data_header) => data_header.serialize(buffer),
            BlobHeader::Coding(ref coding_header) => coding_header.serialize(buffer),
        }
    }

    pub fn deserialize(buffer: &[u8]) -> Option<BlobHeader> {
        if buffer.len() >= BLOB_HEADER_SIZE {
            let flags = LittleEndian::read_u32(&buffer[FLAGS_RANGE]);
            let is_coding = flags & BLOB_FLAG_IS_CODING != 0;

            if is_coding {
                let header = CodingHeader {
                    slot: LittleEndian::read_u64(&buffer[SLOT_RANGE]),
                    index: LittleEndian::read_u64(&buffer[INDEX_RANGE]),
                    data_size: LittleEndian::read_u64(&buffer[SIZE_RANGE]),
                    id: Pubkey::new(&buffer[ID_RANGE]),
                };

                Some(BlobHeader::Coding(header))
            } else {
                let is_last = flags & BLOB_FLAG_IS_LAST_IN_SLOT != 0;
                let should_forward = buffer[FORWARDED_RANGE][0] & 0x1 == 0;

                let header = DataHeader {
                    parent: LittleEndian::read_u64(&buffer[PARENT_RANGE]),
                    slot: LittleEndian::read_u64(&buffer[SLOT_RANGE]),
                    index: LittleEndian::read_u64(&buffer[INDEX_RANGE]),
                    id: Pubkey::new(&buffer[ID_RANGE]),
                    genesis: Hash::new(&buffer[GENESIS_RANGE]),
                    should_forward,
                    is_last_in_slot: is_last,
                    data_size: LittleEndian::read_u64(&buffer[SIZE_RANGE]),
                };

                Some(BlobHeader::Data(header))
            }
        } else {
            None
        }
    }
}

impl CodingBlob {
    pub fn into_blob(mut self) -> Blob {
        self.header.serialize(self.blob.header_mut());
        self.blob
    }

    pub fn data(&self) -> &[u8] {
        self.blob.data()
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        self.blob.data_mut()
    }

    pub fn meta(&self) -> &Meta {
        &self.blob.meta
    }

    pub fn meta_mut(&mut self) -> &mut Meta {
        &mut self.blob.meta
    }
}

impl DataBlob {
    pub fn into_blob(mut self) -> Blob {
        self.header.serialize(self.blob.header_mut());
        self.blob
    }

    pub fn data(&self) -> &[u8] {
        &self.blob.data()[..BLOB_DATA_SIZE]
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.blob.data_mut()[..BLOB_DATA_SIZE]
    }

    pub fn meta(&self) -> &Meta {
        &self.blob.meta
    }

    pub fn meta_mut(&mut self) -> &mut Meta {
        &mut self.blob.meta
    }
}

impl CodingHeader {
    pub fn serialize(&self, buffer: &mut [u8]) {
        assert!(buffer.len() >= BLOB_HEADER_SIZE);

        LittleEndian::write_u64(&mut buffer[SLOT_RANGE], self.slot);
        LittleEndian::write_u64(&mut buffer[INDEX_RANGE], self.index);
        buffer[ID_RANGE].copy_from_slice(self.id.as_ref());
        LittleEndian::write_u32(&mut buffer[FLAGS_RANGE], BLOB_FLAG_IS_CODING);
        LittleEndian::write_u64(&mut buffer[SIZE_RANGE], self.data_size);
    }
}

impl DataHeader {
    pub fn serialize(&self, buffer: &mut [u8]) {
        assert!(buffer.len() >= BLOB_HEADER_SIZE);

        LittleEndian::write_u64(&mut buffer[PARENT_RANGE], self.parent);
        LittleEndian::write_u64(&mut buffer[SLOT_RANGE], self.slot);
        LittleEndian::write_u64(&mut buffer[INDEX_RANGE], self.index);

        buffer[ID_RANGE].copy_from_slice(self.id.as_ref());
        buffer[FORWARDED_RANGE][0] = u8::from(!self.should_forward);
        buffer[GENESIS_RANGE].copy_from_slice(self.genesis.as_ref());

        let flags = if self.is_last_in_slot {
            BLOB_FLAG_IS_LAST_IN_SLOT
        } else {
            0
        };

        LittleEndian::write_u32(&mut buffer[FLAGS_RANGE], flags);
        LittleEndian::write_u64(&mut buffer[SIZE_RANGE], self.data_size);
    }
}

pub fn index_blobs(blobs: &[SharedBlob], id: &Pubkey, blob_index: u64, slot: u64, parent: u64) {
    index_blobs_with_genesis(blobs, id, &Hash::default(), blob_index, slot, parent)
}

pub fn index_blobs_with_genesis(
    blobs: &[SharedBlob],
    id: &Pubkey,
    genesis: &Hash,
    mut blob_index: u64,
    slot: u64,
    parent: u64,
) {
    // enumerate all the blobs, those are the indices
    for blob in blobs.iter() {
        let mut blob = blob.write().unwrap();

        blob.set_index(blob_index);
        blob.set_genesis_blockhash(genesis);
        blob.set_slot(slot);
        blob.set_parent(parent);
        blob.set_id(id);
        blob_index += 1;
    }
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

pub fn packets_to_blobs<T: Borrow<Packet>>(packets: &[T]) -> Vec<Blob> {
    let mut current_index = 0;
    let mut blobs = vec![];
    while current_index < packets.len() {
        let mut blob = Blob::default();
        current_index += blob.store_packets(&packets[current_index..]) as usize;
        blobs.push(blob);
    }

    blobs
}

pub fn deserialize_packets_in_blob(
    data: &[u8],
    serialized_packet_size: usize,
    serialized_meta_size: usize,
) -> Result<Vec<Packet>> {
    let mut packets: Vec<Packet> = Vec::with_capacity(data.len() / serialized_packet_size);
    let mut pos = 0;
    while pos + serialized_packet_size <= data.len() {
        let packet = deserialize_single_packet_in_blob(
            &data[pos..pos + serialized_packet_size],
            serialized_meta_size,
        )?;
        pos += serialized_packet_size;
        packets.push(packet);
    }
    Ok(packets)
}

fn deserialize_single_packet_in_blob(data: &[u8], serialized_meta_size: usize) -> Result<Packet> {
    let meta = bincode::deserialize(&data[..serialized_meta_size])?;
    let mut packet_data = [0; PACKET_DATA_SIZE];
    packet_data
        .copy_from_slice(&data[serialized_meta_size..serialized_meta_size + PACKET_DATA_SIZE]);
    Ok(Packet::new(packet_data, meta))
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
        self.data.as_ref() == other.data.as_ref()
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

impl From<CodingBlob> for Blob {
    fn from(coding: CodingBlob) -> Blob {
        coding.into_blob()
    }
}

impl From<DataBlob> for Blob {
    fn from(data: DataBlob) -> Blob {
        data.into_blob()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode;
    use rand::Rng;
    use solana_sdk::hash::Hash;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;

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
    fn test_store_blobs_max() {
        let meta = Meta::default();
        let serialized_meta_size = bincode::serialized_size(&meta).unwrap() as usize;
        let serialized_packet_size = serialized_meta_size + PACKET_DATA_SIZE;
        let num_packets = (BLOB_SIZE - BLOB_HEADER_SIZE) / serialized_packet_size + 1;
        let mut blob = Blob::default();
        let packets: Vec<_> = (0..num_packets).map(|_| Packet::default()).collect();

        // Everything except the last packet should have been written
        assert_eq!(blob.store_packets(&packets[..]), (num_packets - 1) as u64);

        blob = Blob::default();
        // Store packets such that blob only has room for one more
        assert_eq!(
            blob.store_packets(&packets[..num_packets - 2]),
            (num_packets - 2) as u64
        );

        // Fill the last packet in the blob
        assert_eq!(blob.store_packets(&packets[..num_packets - 2]), 1);

        // Blob is now full
        assert_eq!(blob.store_packets(&packets), 0);
    }

    #[test]
    fn test_packets_to_blobs() {
        let mut rng = rand::thread_rng();
        let meta = Meta::default();
        let serialized_meta_size = bincode::serialized_size(&meta).unwrap() as usize;
        let serialized_packet_size = serialized_meta_size + PACKET_DATA_SIZE;

        let packets_per_blob = (BLOB_SIZE - BLOB_HEADER_SIZE) / serialized_packet_size;
        assert!(packets_per_blob > 1);
        let num_packets = packets_per_blob * 10 + packets_per_blob - 1;

        let packets: Vec<_> = (0..num_packets)
            .map(|_| {
                let mut packet = Packet::default();
                for i in 0..packet.meta.addr.len() {
                    packet.meta.addr[i] = rng.gen_range(1, std::u16::MAX);
                }
                for i in 0..packet.data.len() {
                    packet.data[i] = rng.gen_range(1, std::u8::MAX);
                }
                packet
            })
            .collect();

        let blobs = packets_to_blobs(&packets[..]);
        assert_eq!(blobs.len(), 11);

        let reconstructed_packets: Vec<Packet> = blobs
            .iter()
            .flat_map(|b| {
                deserialize_packets_in_blob(
                    &b.data()[..b.size()],
                    serialized_packet_size,
                    serialized_meta_size,
                )
                .unwrap()
            })
            .collect();

        assert_eq!(reconstructed_packets, packets);
    }

    #[test]
    fn test_deserialize_packets_in_blob() {
        let meta = Meta::default();
        let serialized_meta_size = bincode::serialized_size(&meta).unwrap() as usize;
        let serialized_packet_size = serialized_meta_size + PACKET_DATA_SIZE;
        let num_packets = 10;
        let mut rng = rand::thread_rng();
        let packets: Vec<_> = (0..num_packets)
            .map(|_| {
                let mut packet = Packet::default();
                for i in 0..packet.meta.addr.len() {
                    packet.meta.addr[i] = rng.gen_range(1, std::u16::MAX);
                }
                for i in 0..packet.data.len() {
                    packet.data[i] = rng.gen_range(1, std::u8::MAX);
                }
                packet
            })
            .collect();

        let mut blob = Blob::default();
        assert_eq!(blob.store_packets(&packets[..]), num_packets);
        let result = deserialize_packets_in_blob(
            &blob.data()[..blob.size()],
            serialized_packet_size,
            serialized_meta_size,
        )
        .unwrap();

        assert_eq!(result, packets);
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
    fn test_blob_genesis_blockhash() {
        let mut blob = Blob::default();
        assert_eq!(blob.genesis_blockhash(), Hash::default());

        let hash = Hash::new(&Pubkey::new_rand().as_ref());
        blob.set_genesis_blockhash(&hash);
        assert_eq!(blob.genesis_blockhash(), hash);
    }

    #[test]
    fn test_header_serialization() {
        use crate::entry::make_consecutive_blobs;
        use crate::erasure::{CodingGenerator, NUM_DATA};

        const N_BLOBS: u64 = 64;

        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let blobs = make_consecutive_blobs(&Pubkey::default(), N_BLOBS, 0, Hash::default(), &addr);
        let mut serialized_header = vec![0; BLOB_HEADER_SIZE];

        // make sure length is checked
        {
            let blob = blobs[0].read().unwrap();
            let header = blob.header();

            assert!(BlobHeader::deserialize(header).is_some());
            assert!(BlobHeader::deserialize(&header[..BLOB_HEADER_SIZE - 1]).is_none());
        }

        for blobs in blobs.chunks(NUM_DATA) {
            // test data blob round-trip
            for blob in blobs {
                let blob = blob.read().unwrap();

                if let BlobHeader::Data(header) = blob.read_header() {
                    assert_eq!(header.parent, blob.parent());
                    assert_eq!(header.slot, blob.slot());
                    assert_eq!(header.index, blob.index());
                    assert_eq!(header.id, blob.id());
                    assert_eq!(header.should_forward, blob.should_forward());
                    assert_eq!(header.genesis, blob.genesis_blockhash());
                    assert_eq!(header.is_last_in_slot, blob.is_last_in_slot());
                    assert_eq!(header.data_size, blob.data_size());

                    header.serialize(&mut serialized_header);

                    assert_eq!(serialized_header, blob.header());
                } else {
                    panic!("Data blob header deserialized into coding blob header");
                }
            }

            // test coding blob round-trip
            let mut coding_generator = CodingGenerator::default();
            let coding_blobs = coding_generator.next(blobs);

            for blob in coding_blobs {
                let blob = blob.read().unwrap();

                if let BlobHeader::Coding(header) = blob.read_header() {
                    assert_eq!(header.slot, blob.slot());
                    assert_eq!(header.index, blob.index());
                    assert_eq!(header.id, blob.id());
                    assert_eq!(header.data_size, blob.data_size());

                    header.serialize(&mut serialized_header);

                    assert_eq!(serialized_header, blob.header());
                } else {
                    panic!("Coding blob header deserialized into data blob header");
                }
            }
        }
    }

    #[test]
    fn test_blob_round_trip() {
        use crate::entry::make_consecutive_blobs;
        use crate::erasure::{CodingGenerator, NUM_DATA};

        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let blobs = make_consecutive_blobs(
            &Pubkey::default(),
            NUM_DATA as u64,
            0,
            Hash::default(),
            &addr,
        );
        let coding_blobs = CodingGenerator::default().next(&blobs);

        for blob in blobs {
            let blob = blob.read().unwrap();

            if let BlobHeader::Data(header) = blob.read_header() {
                let mut data_blob = blob.clone().into_data(header);

                assert_eq!(data_blob.data(), &blob.data()[..BLOB_DATA_SIZE]);
                assert_eq!(data_blob.meta(), &blob.meta);

                // make sure changes persist across serialization
                assert_eq!(&data_blob.clone().into_blob(), &*blob);
                data_blob.header.index += 1;
                assert_ne!(&data_blob.into_blob(), &*blob);
            } else {
                panic!("Data blobs must be `DataBlob`s");
            }
        }

        for blob in coding_blobs {
            let blob = blob.read().unwrap();

            if let BlobHeader::Coding(header) = blob.read_header() {
                let mut coding_blob = blob.clone().into_coding(header);

                assert_eq!(coding_blob.data(), blob.data());
                assert_eq!(coding_blob.meta(), &blob.meta);

                // make sure changes persist across serialization
                assert_eq!(&coding_blob.clone().into_blob(), &*blob);
                coding_blob.header.index += 1;
                assert_ne!(&coding_blob.into_blob(), &*blob);
            } else {
                panic!("Coding blobs must be `CodingBlob`s");
            }
        }
    }
}
