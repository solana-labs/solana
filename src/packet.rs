//! The `packet` module defines data structures and methods to pull data from the network.
use bincode::{deserialize, serialize};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use counter::Counter;
#[cfg(test)]
use entry::Entry;
#[cfg(test)]
use ledger::Block;
use log::Level;
use recvmmsg::{recv_mmsg, NUM_RCVMMSGS};
use result::{Error, Result};
use serde::Serialize;
#[cfg(test)]
use solana_sdk::hash::Hash;
pub use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::pubkey::Pubkey;
use std::fmt;
use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

pub type SharedPackets = Arc<RwLock<Packets>>;
pub type SharedBlob = Arc<RwLock<Blob>>;
pub type SharedBlobs = Vec<SharedBlob>;

pub const NUM_PACKETS: usize = 1024 * 8;
pub const BLOB_SIZE: usize = (64 * 1024 - 128); // wikipedia says there should be 20b for ipv4 headers
pub const BLOB_DATA_SIZE: usize = BLOB_SIZE - (BLOB_HEADER_SIZE * 2);
pub const NUM_BLOBS: usize = (NUM_PACKETS * PACKET_DATA_SIZE) / BLOB_SIZE;

#[derive(Clone, Default, Debug, PartialEq)]
#[repr(C)]
pub struct Meta {
    pub size: usize,
    pub num_retransmits: u64,
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

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
            data: [0u8; PACKET_DATA_SIZE],
            meta: Meta::default(),
        }
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

#[derive(Debug)]
pub struct Packets {
    pub packets: Vec<Packet>,
}

//auto derive doesn't support large arrays
impl Default for Packets {
    fn default() -> Packets {
        Packets {
            packets: vec![Packet::default(); NUM_PACKETS],
        }
    }
}

#[derive(Clone)]
pub struct Blob {
    pub data: [u8; BLOB_SIZE],
    pub meta: Meta,
}

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Blob {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.meta.addr()
        )
    }
}

//auto derive doesn't support large arrays
impl Default for Blob {
    fn default() -> Blob {
        Blob {
            data: [0u8; BLOB_SIZE],
            meta: Meta::default(),
        }
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
    fn run_read_from(&mut self, socket: &UdpSocket) -> Result<usize> {
        self.packets.resize(NUM_PACKETS, Packet::default());
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
            match recv_mmsg(socket, &mut self.packets[i..]) {
                Err(_) if i > 0 => {
                    inc_new_counter_info!("packets-recv_count", i);
                    debug!("got {:?} messages on {}", i, socket.local_addr().unwrap());
                    socket.set_nonblocking(true)?;
                    return Ok(i);
                }
                Err(e) => {
                    trace!("recv_from err {:?}", e);
                    return Err(Error::IO(e));
                }
                Ok(npkts) => {
                    trace!("got {} packets", npkts);
                    i += npkts;
                    if npkts != NUM_RCVMMSGS {
                        socket.set_nonblocking(true)?;
                        inc_new_counter_info!("packets-recv_count", i);
                        return Ok(i);
                    }
                }
            }
        }
    }
    pub fn recv_from(&mut self, socket: &UdpSocket) -> Result<()> {
        let sz = self.run_read_from(socket)?;
        self.packets.resize(sz, Packet::default());
        debug!("recv_from: {}", sz);
        Ok(())
    }
    pub fn send_to(&self, socket: &UdpSocket) -> Result<()> {
        for p in &self.packets {
            let a = p.meta.addr();
            socket.send_to(&p.data[..p.meta.size], &a)?;
        }
        Ok(())
    }
}

pub fn to_packets_chunked<T: Serialize>(xs: &[T], chunks: usize) -> Vec<SharedPackets> {
    let mut out = vec![];
    for x in xs.chunks(chunks) {
        let mut p = SharedPackets::default();
        p.write()
            .unwrap()
            .packets
            .resize(x.len(), Default::default());
        for (i, o) in x.iter().zip(p.write().unwrap().packets.iter_mut()) {
            let v = serialize(&i).expect("serialize request");
            let len = v.len();
            o.data[..len].copy_from_slice(&v);
            o.meta.size = len;
        }
        out.push(p);
    }
    out
}

pub fn to_packets<T: Serialize>(xs: &[T]) -> Vec<SharedPackets> {
    to_packets_chunked(xs, NUM_PACKETS)
}

pub fn to_blob<T: Serialize>(resp: T, rsp_addr: SocketAddr) -> Result<SharedBlob> {
    let blob = SharedBlob::default();
    {
        let mut b = blob.write().unwrap();
        let v = serialize(&resp)?;
        let len = v.len();
        assert!(len <= BLOB_SIZE);
        b.data[..len].copy_from_slice(&v);
        b.meta.size = len;
        b.meta.set_addr(&rsp_addr);
    }
    Ok(blob)
}

pub fn to_blobs<T: Serialize>(rsps: Vec<(T, SocketAddr)>) -> Result<SharedBlobs> {
    let mut blobs = Vec::new();
    for (resp, rsp_addr) in rsps {
        blobs.push(to_blob(resp, rsp_addr)?);
    }
    Ok(blobs)
}

const BLOB_SLOT_END: usize = size_of::<u64>();
const BLOB_INDEX_END: usize = BLOB_SLOT_END + size_of::<u64>();
const BLOB_ID_END: usize = BLOB_INDEX_END + size_of::<Pubkey>();
const BLOB_FLAGS_END: usize = BLOB_ID_END + size_of::<u32>();
const BLOB_SIZE_END: usize = BLOB_FLAGS_END + size_of::<u64>();

macro_rules! align {
    ($x:expr, $align:expr) => {
        $x + ($align - 1) & !($align - 1)
    };
}

pub const BLOB_FLAG_IS_CODING: u32 = 0x1;
pub const BLOB_HEADER_SIZE: usize = align!(BLOB_SIZE_END, 64);

impl Blob {
    pub fn slot(&self) -> Result<u64> {
        let mut rdr = io::Cursor::new(&self.data[0..BLOB_SLOT_END]);
        let r = rdr.read_u64::<LittleEndian>()?;
        Ok(r)
    }
    pub fn set_slot(&mut self, ix: u64) -> Result<()> {
        let mut wtr = vec![];
        wtr.write_u64::<LittleEndian>(ix)?;
        self.data[..BLOB_SLOT_END].clone_from_slice(&wtr);
        Ok(())
    }
    pub fn index(&self) -> Result<u64> {
        let mut rdr = io::Cursor::new(&self.data[BLOB_SLOT_END..BLOB_INDEX_END]);
        let r = rdr.read_u64::<LittleEndian>()?;
        Ok(r)
    }
    pub fn set_index(&mut self, ix: u64) -> Result<()> {
        let mut wtr = vec![];
        wtr.write_u64::<LittleEndian>(ix)?;
        self.data[BLOB_SLOT_END..BLOB_INDEX_END].clone_from_slice(&wtr);
        Ok(())
    }
    /// sender id, we use this for identifying if its a blob from the leader that we should
    /// retransmit.  eventually blobs should have a signature that we can use ffor spam filtering
    pub fn id(&self) -> Result<Pubkey> {
        let e = deserialize(&self.data[BLOB_INDEX_END..BLOB_ID_END])?;
        Ok(e)
    }

    pub fn set_id(&mut self, id: &Pubkey) -> Result<()> {
        let wtr = serialize(id)?;
        self.data[BLOB_INDEX_END..BLOB_ID_END].clone_from_slice(&wtr);
        Ok(())
    }

    pub fn flags(&self) -> Result<u32> {
        let mut rdr = io::Cursor::new(&self.data[BLOB_ID_END..BLOB_FLAGS_END]);
        let r = rdr.read_u32::<LittleEndian>()?;
        Ok(r)
    }

    pub fn set_flags(&mut self, ix: u32) -> Result<()> {
        let mut wtr = vec![];
        wtr.write_u32::<LittleEndian>(ix)?;
        self.data[BLOB_ID_END..BLOB_FLAGS_END].clone_from_slice(&wtr);
        Ok(())
    }

    pub fn is_coding(&self) -> bool {
        (self.flags().unwrap() & BLOB_FLAG_IS_CODING) != 0
    }

    pub fn set_coding(&mut self) -> Result<()> {
        let flags = self.flags().unwrap();
        self.set_flags(flags | BLOB_FLAG_IS_CODING)
    }

    pub fn data_size(&self) -> Result<u64> {
        let mut rdr = io::Cursor::new(&self.data[BLOB_FLAGS_END..BLOB_SIZE_END]);
        let r = rdr.read_u64::<LittleEndian>()?;
        Ok(r)
    }

    pub fn set_data_size(&mut self, ix: u64) -> Result<()> {
        let mut wtr = vec![];
        wtr.write_u64::<LittleEndian>(ix)?;
        self.data[BLOB_FLAGS_END..BLOB_SIZE_END].clone_from_slice(&wtr);
        Ok(())
    }

    pub fn data(&self) -> &[u8] {
        &self.data[BLOB_HEADER_SIZE..]
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data[BLOB_HEADER_SIZE..]
    }
    pub fn size(&self) -> Result<usize> {
        let size = self.data_size()? as usize;
        if self.meta.size == size {
            Ok(size - BLOB_HEADER_SIZE)
        } else {
            Err(Error::BlobError(BlobError::BadState))
        }
    }
    pub fn set_size(&mut self, size: usize) {
        let new_size = size + BLOB_HEADER_SIZE;
        self.meta.size = new_size;
        self.set_data_size(new_size as u64).unwrap();
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
}

pub fn index_blobs(blobs: &[SharedBlob], id: &Pubkey, mut index: u64, slot: u64) {
    // enumerate all the blobs, those are the indices
    for b in blobs {
        let mut blob = b.write().unwrap();

        blob.set_index(index).expect("set_index");
        blob.set_slot(slot).expect("set_slot");
        blob.set_id(id).expect("set_id");
        blob.set_flags(0).unwrap();

        index += 1;
    }
}

#[cfg(test)]
pub fn make_consecutive_blobs(
    me_id: Pubkey,
    num_blobs_to_make: u64,
    start_height: u64,
    start_hash: Hash,
    addr: &SocketAddr,
) -> SharedBlobs {
    let mut last_hash = start_hash;
    let num_hashes = 1;
    let mut all_entries = Vec::with_capacity(num_blobs_to_make as usize);
    for _ in 0..num_blobs_to_make {
        let entry = Entry::new(&last_hash, num_hashes, vec![]);
        last_hash = entry.id;
        all_entries.push(entry);
    }
    let mut new_blobs = all_entries.to_blobs_with_id(me_id, start_height, addr);
    new_blobs.truncate(num_blobs_to_make as usize);
    new_blobs
}

#[cfg(test)]
mod tests {
    use packet::{
        to_packets, Blob, Meta, Packet, Packets, SharedBlob, SharedPackets, NUM_PACKETS,
        PACKET_DATA_SIZE,
    };
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::hash::Hash;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    #[test]
    pub fn packet_send_recv() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = sender.local_addr().unwrap();
        let p = SharedPackets::default();
        p.write().unwrap().packets.resize(10, Packet::default());
        for m in p.write().unwrap().packets.iter_mut() {
            m.meta.set_addr(&addr);
            m.meta.size = PACKET_DATA_SIZE;
        }
        p.read().unwrap().send_to(&sender).unwrap();
        p.write().unwrap().recv_from(&reader).unwrap();
        for m in p.write().unwrap().packets.iter_mut() {
            assert_eq!(m.meta.size, PACKET_DATA_SIZE);
            assert_eq!(m.meta.addr(), saddr);
        }
    }

    #[test]
    fn test_to_packets() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = Transaction::system_new(&keypair, keypair.pubkey(), 1, hash);
        let rv = to_packets(&vec![tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), 1);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].read().unwrap().packets.len(), 1);
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
        b.set_index(<u64>::max_value()).unwrap();
        assert_eq!(b.index().unwrap(), <u64>::max_value());
        b.data_mut()[0] = 1;
        assert_eq!(b.data()[0], 1);
        assert_eq!(b.index().unwrap(), <u64>::max_value());
        assert_eq!(b.meta, Meta::default());
    }

}
