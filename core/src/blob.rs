//! The `packet` module defines data structures and methods to pull data from the network.
use crate::{
    packet::NUM_PACKETS,
    result::{Error, Result},
};
use bincode;
use byteorder::{ByteOrder, LittleEndian};
use serde::Serialize;
pub use solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signable, Signature},
};
use std::{
    borrow::Cow,
    cmp, fmt, io,
    io::Cursor,
    mem::size_of,
    net::{SocketAddr, UdpSocket},
    ops::{Deref, DerefMut},
    sync::{Arc, RwLock},
};

pub type SharedBlob = Arc<RwLock<Blob>>;
pub type SharedBlobs = Vec<SharedBlob>;

pub const BLOB_SIZE: usize = (2 * 1024 - 128); // wikipedia says there should be 20b for ipv4 headers
pub const BLOB_DATA_SIZE: usize = BLOB_SIZE - (BLOB_HEADER_SIZE * 2);
pub const BLOB_DATA_ALIGN: usize = 16; // safe for erasure input pointers, gf.c needs 16byte-aligned buffers
pub const NUM_BLOBS: usize = (NUM_PACKETS * PACKET_DATA_SIZE) / BLOB_SIZE;

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

pub fn to_blob<T: Serialize>(resp: T, rsp_addr: SocketAddr) -> Result<Blob> {
    let mut b = Blob::default();
    let v = bincode::serialize(&resp)?;
    let len = v.len();
    if len > BLOB_SIZE {
        return Err(Error::ToBlobError);
    }
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
const ID_RANGE: std::ops::Range<usize> = range!(SIGNATURE_RANGE.end, Pubkey);
const SIZE_RANGE: std::ops::Range<usize> = range!(ID_RANGE.end, u64);

macro_rules! align {
    ($x:expr, $align:expr) => {
        $x + ($align - 1) & !($align - 1)
    };
}

pub const BLOB_HEADER_SIZE: usize = align!(SIZE_RANGE.end, BLOB_DATA_ALIGN); // make sure data() is safe for erasure
pub const SIGNABLE_START: usize = SIGNATURE_RANGE.end;

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
        blob
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
                    return Err(e.into());
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

pub fn limited_deserialize<T>(data: &[u8]) -> bincode::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    bincode::config().limit(BLOB_SIZE as u64).deserialize(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::{Keypair, KeypairUtil};
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
    pub fn blob_test() {
        let mut b = Blob::default();
        b.data_mut()[0] = 1;
        assert_eq!(b.data()[0], 1);
        assert_eq!(b.meta, Meta::default());
    }

    #[test]
    fn test_blob_data_align() {
        assert_eq!(std::mem::align_of::<BlobData>(), BLOB_DATA_ALIGN);
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
    pub fn debug_trait() {
        write!(io::sink(), "{:?}", Blob::default()).unwrap();
    }
}
