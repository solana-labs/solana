//! The `packet` module defines data structures and methods to pull data from the network.
use bincode::{deserialize, serialize};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use counter::Counter;
use result::{Error, Result};
use serde::Serialize;
use signature::PublicKey;
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};

pub type SharedPackets = Arc<RwLock<Packets>>;
pub type SharedBlob = Arc<RwLock<Blob>>;
pub type SharedBlobs = VecDeque<SharedBlob>;
pub type PacketRecycler = Recycler<Packets>;
pub type BlobRecycler = Recycler<Blob>;

pub const NUM_PACKETS: usize = 1024 * 8;
pub const BLOB_SIZE: usize = 64 * 1024;
pub const BLOB_DATA_SIZE: usize = BLOB_SIZE - BLOB_HEADER_SIZE;
pub const PACKET_DATA_SIZE: usize = 256;
pub const NUM_BLOBS: usize = (NUM_PACKETS * PACKET_DATA_SIZE) / BLOB_SIZE;

#[derive(Clone, Default)]
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
                self.port = a.port();
            }
            SocketAddr::V6(v6) => {
                self.addr = v6.ip().segments();
                self.port = a.port();
                self.v6 = true;
            }
        }
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

pub struct Recycler<T> {
    gc: Arc<Mutex<Vec<Arc<RwLock<T>>>>>,
}

impl<T: Default> Default for Recycler<T> {
    fn default() -> Recycler<T> {
        Recycler {
            gc: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl<T: Default> Clone for Recycler<T> {
    fn clone(&self) -> Recycler<T> {
        Recycler {
            gc: self.gc.clone(),
        }
    }
}

impl<T: Default> Recycler<T> {
    pub fn allocate(&self) -> Arc<RwLock<T>> {
        let mut gc = self.gc.lock().expect("recycler lock in pb fn allocate");
        let x = gc.pop()
            .unwrap_or_else(|| Arc::new(RwLock::new(Default::default())));

        // Only return the item if this recycler is the last reference to it.
        // Remove this check once `T` holds a Weak reference back to this
        // recycler and implements `Drop`. At the time of this writing, Weak can't
        // be passed across threads ('alloc' is a nightly-only API), and so our
        // reference-counted recyclables are awkwardly being recycled by hand,
        // which allows this race condition to exist.
        if Arc::strong_count(&x) > 1 {
            warn!("Recycled item still in use. Booting it.");
            drop(gc);
            self.allocate()
        } else {
            x
        }
    }
    pub fn recycle(&self, x: Arc<RwLock<T>>) {
        let mut gc = self.gc.lock().expect("recycler lock in pub fn recycle");
        gc.push(x);
    }
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
        for p in &mut self.packets {
            p.meta.size = 0;
            trace!("receiving on {}", socket.local_addr().unwrap());
            match socket.recv_from(&mut p.data) {
                Err(_) if i > 0 => {
                    inc_new_counter!("packets-recv_count", 1);
                    debug!("got {:?} messages on {}", i, socket.local_addr().unwrap());
                    break;
                }
                Err(e) => {
                    trace!("recv_from err {:?}", e);
                    return Err(Error::IO(e));
                }
                Ok((nrecv, from)) => {
                    p.meta.size = nrecv;
                    p.meta.set_addr(&from);
                    if i == 0 {
                        socket.set_nonblocking(true)?;
                    }
                }
            }
            i += 1;
        }
        Ok(i)
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

pub fn to_packets_chunked<T: Serialize>(
    r: &PacketRecycler,
    xs: &[T],
    chunks: usize,
) -> Vec<SharedPackets> {
    let mut out = vec![];
    for x in xs.chunks(chunks) {
        let p = r.allocate();
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

pub fn to_packets<T: Serialize>(r: &PacketRecycler, xs: &[T]) -> Vec<SharedPackets> {
    to_packets_chunked(r, xs, NUM_PACKETS)
}

pub fn to_blob<T: Serialize>(
    resp: T,
    rsp_addr: SocketAddr,
    blob_recycler: &BlobRecycler,
) -> Result<SharedBlob> {
    let blob = blob_recycler.allocate();
    {
        let mut b = blob.write().unwrap();
        let v = serialize(&resp)?;
        let len = v.len();
        assert!(len < BLOB_SIZE);
        b.data[..len].copy_from_slice(&v);
        b.meta.size = len;
        b.meta.set_addr(&rsp_addr);
    }
    Ok(blob)
}

pub fn to_blobs<T: Serialize>(
    rsps: Vec<(T, SocketAddr)>,
    blob_recycler: &BlobRecycler,
) -> Result<SharedBlobs> {
    let mut blobs = VecDeque::new();
    for (resp, rsp_addr) in rsps {
        blobs.push_back(to_blob(resp, rsp_addr, blob_recycler)?);
    }
    Ok(blobs)
}

const BLOB_INDEX_END: usize = size_of::<u64>();
const BLOB_ID_END: usize = BLOB_INDEX_END + size_of::<usize>() + size_of::<PublicKey>();
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
    pub fn get_index(&self) -> Result<u64> {
        let mut rdr = io::Cursor::new(&self.data[0..BLOB_INDEX_END]);
        let r = rdr.read_u64::<LittleEndian>()?;
        Ok(r)
    }
    pub fn set_index(&mut self, ix: u64) -> Result<()> {
        let mut wtr = vec![];
        wtr.write_u64::<LittleEndian>(ix)?;
        self.data[..BLOB_INDEX_END].clone_from_slice(&wtr);
        Ok(())
    }
    /// sender id, we use this for identifying if its a blob from the leader that we should
    /// retransmit.  eventually blobs should have a signature that we can use ffor spam filtering
    pub fn get_id(&self) -> Result<PublicKey> {
        let e = deserialize(&self.data[BLOB_INDEX_END..BLOB_ID_END])?;
        Ok(e)
    }

    pub fn set_id(&mut self, id: PublicKey) -> Result<()> {
        let wtr = serialize(&id)?;
        self.data[BLOB_INDEX_END..BLOB_ID_END].clone_from_slice(&wtr);
        Ok(())
    }

    pub fn get_flags(&self) -> Result<u32> {
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
        (self.get_flags().unwrap() & BLOB_FLAG_IS_CODING) != 0
    }

    pub fn set_coding(&mut self) -> Result<()> {
        let flags = self.get_flags().unwrap();
        self.set_flags(flags | BLOB_FLAG_IS_CODING)
    }

    pub fn get_data_size(&self) -> Result<u64> {
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
    pub fn set_size(&mut self, size: usize) {
        let new_size = size + BLOB_HEADER_SIZE;
        self.meta.size = new_size;
        self.set_data_size(new_size as u64).unwrap();
    }
    pub fn recv_from(re: &BlobRecycler, socket: &UdpSocket) -> Result<SharedBlobs> {
        let mut v = VecDeque::new();
        //DOCUMENTED SIDE-EFFECT
        //Performance out of the IO without poll
        //  * block on the socket until it's readable
        //  * set the socket to non blocking
        //  * read until it fails
        //  * set it back to blocking before returning
        socket.set_nonblocking(false)?;
        for i in 0..NUM_BLOBS {
            let r = re.allocate();
            {
                let mut p = r.write().expect("'r' write lock in pub fn recv_from");
                trace!("receiving on {}", socket.local_addr().unwrap());
                match socket.recv_from(&mut p.data) {
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
                    Ok((nrecv, from)) => {
                        p.meta.size = nrecv;
                        p.meta.set_addr(&from);
                        if i == 0 {
                            socket.set_nonblocking(true)?;
                        }
                    }
                }
            }
            v.push_back(r);
        }
        Ok(v)
    }
    pub fn send_to(re: &BlobRecycler, socket: &UdpSocket, v: &mut SharedBlobs) -> Result<()> {
        while let Some(r) = v.pop_front() {
            {
                let p = r.read().expect("'r' read lock in pub fn send_to");
                let a = p.meta.addr();
                if let Err(e) = socket.send_to(&p.data[..p.meta.size], &a) {
                    info!(
                        "error sending {} byte packet to {:?}: {:?}",
                        p.meta.size, a, e
                    );
                    Err(e)?;
                }
            }
            re.recycle(r);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use packet::{
        to_packets, Blob, BlobRecycler, Packet, PacketRecycler, Packets, Recycler, NUM_PACKETS,
    };
    use request::Request;
    use std::collections::VecDeque;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::Arc;

    #[test]
    pub fn packet_recycler_test() {
        let r = PacketRecycler::default();
        let p = r.allocate();
        r.recycle(p);
        assert_eq!(r.gc.lock().unwrap().len(), 1);
        let _ = r.allocate();
        assert_eq!(r.gc.lock().unwrap().len(), 0);
    }

    #[test]
    pub fn test_leaked_recyclable() {
        // Ensure that the recycler won't return an item
        // that is still referenced outside the recycler.
        let r = Recycler::<u8>::default();
        let x0 = r.allocate();
        r.recycle(x0.clone());
        assert_eq!(Arc::strong_count(&x0), 2);
        assert_eq!(r.gc.lock().unwrap().len(), 1);

        let x1 = r.allocate();
        assert_eq!(Arc::strong_count(&x1), 1);
        assert_eq!(r.gc.lock().unwrap().len(), 0);
    }

    #[test]
    pub fn test_leaked_recyclable_recursion() {
        // In the case of a leaked recyclable, ensure the recycler drops its lock before recursing.
        let r = Recycler::<u8>::default();
        let x0 = r.allocate();
        let x1 = r.allocate();
        r.recycle(x0); // <-- allocate() of this will require locking the recycler's stack.
        r.recycle(x1.clone()); // <-- allocate() of this will cause it to be dropped and recurse.
        assert_eq!(Arc::strong_count(&x1), 2);
        assert_eq!(r.gc.lock().unwrap().len(), 2);

        r.allocate(); // Ensure lock is released before recursing.
        assert_eq!(r.gc.lock().unwrap().len(), 0);
    }

    #[test]
    pub fn blob_recycler_test() {
        let r = BlobRecycler::default();
        let p = r.allocate();
        r.recycle(p);
        assert_eq!(r.gc.lock().unwrap().len(), 1);
        let _ = r.allocate();
        assert_eq!(r.gc.lock().unwrap().len(), 0);
    }
    #[test]
    pub fn packet_send_recv() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let saddr = sender.local_addr().unwrap();
        let r = PacketRecycler::default();
        let p = r.allocate();
        p.write().unwrap().packets.resize(10, Packet::default());
        for m in p.write().unwrap().packets.iter_mut() {
            m.meta.set_addr(&addr);
            m.meta.size = 256;
        }
        p.read().unwrap().send_to(&sender).unwrap();
        p.write().unwrap().recv_from(&reader).unwrap();
        for m in p.write().unwrap().packets.iter_mut() {
            assert_eq!(m.meta.size, 256);
            assert_eq!(m.meta.addr(), saddr);
        }

        r.recycle(p);
    }

    #[test]
    fn test_to_packets() {
        let tx = Request::GetTransactionCount;
        let re = PacketRecycler::default();
        let rv = to_packets(&re, &vec![tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), 1);

        let rv = to_packets(&re, &vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);

        let rv = to_packets(&re, &vec![tx.clone(); NUM_PACKETS + 1]);
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
        let r = BlobRecycler::default();
        let p = r.allocate();
        p.write().unwrap().meta.set_addr(&addr);
        p.write().unwrap().meta.size = 1024;
        let mut v = VecDeque::new();
        v.push_back(p);
        assert_eq!(v.len(), 1);
        Blob::send_to(&r, &sender, &mut v).unwrap();
        trace!("send_to");
        assert_eq!(v.len(), 0);
        let mut rv = Blob::recv_from(&r, &reader).unwrap();
        trace!("recv_from");
        assert_eq!(rv.len(), 1);
        let rp = rv.pop_front().unwrap();
        assert_eq!(rp.write().unwrap().meta.size, 1024);
        r.recycle(rp);
    }

    #[cfg(all(feature = "ipv6", test))]
    #[test]
    pub fn blob_ipv6_send_recv() {
        let reader = UdpSocket::bind("[::1]:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("[::1]:0").expect("bind");
        let r = BlobRecycler::default();
        let p = r.allocate();
        p.write().unwrap().meta.set_addr(&addr);
        p.write().unwrap().meta.size = 1024;
        let mut v = VecDeque::default();
        v.push_back(p);
        Blob::send_to(&r, &sender, &mut v).unwrap();
        let mut rv = Blob::recv_from(&r, &reader).unwrap();
        let rp = rv.pop_front().unwrap();
        assert_eq!(rp.write().unwrap().meta.size, 1024);
        r.recycle(rp);
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
        assert_eq!(b.get_index().unwrap(), <u64>::max_value());
        b.data_mut()[0] = 1;
        assert_eq!(b.data()[0], 1);
        assert_eq!(b.get_index().unwrap(), <u64>::max_value());
    }

}
