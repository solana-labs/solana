use std::sync::{Arc, Mutex, RwLock};
use std::sync::mpsc;
use std::fmt;
use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::thread::{spawn, JoinHandle};
use result::{Error, Result};

const BLOCK_SIZE: usize = 1024 * 8;
pub const PACKET_SIZE: usize = 256;

#[derive(Clone)]
pub struct Packet {
    pub data: [u8; PACKET_SIZE],
    pub size: usize,
    pub addr: [u16; 8],
    pub port: u16,
    pub v6: bool,
}
impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Packet {{ size: {:?}, addr: {:?} }}",
            self.size,
            self.get_addr()
        )
    }
}
impl Default for Packet {
    fn default() -> Packet {
        Packet {
            data: [0u8; PACKET_SIZE],
            size: 0,
            addr: [0u16; 8],
            port: 0,
            v6: false,
        }
    }
}
impl Packet {
    pub fn get_addr(&self) -> SocketAddr {
        if !self.v6 {
            let ipv4 = Ipv4Addr::new(
                self.addr[0] as u8,
                self.addr[1] as u8,
                self.addr[2] as u8,
                self.addr[3] as u8,
            );
            SocketAddr::new(IpAddr::V4(ipv4), self.port)
        } else {
            let ipv6 = Ipv6Addr::new(
                self.addr[0],
                self.addr[1],
                self.addr[2],
                self.addr[3],
                self.addr[4],
                self.addr[5],
                self.addr[6],
                self.addr[7],
            );
            SocketAddr::new(IpAddr::V6(ipv6), self.port)
        }
    }

    pub fn set_addr(&mut self, a: &SocketAddr) {
        match a {
            &SocketAddr::V4(v4) => {
                let ip = v4.ip().octets();
                self.addr[0] = ip[0] as u16;
                self.addr[1] = ip[1] as u16;
                self.addr[2] = ip[2] as u16;
                self.addr[3] = ip[3] as u16;
                self.port = a.port();
            }
            &SocketAddr::V6(v6) => {
                self.addr = v6.ip().segments();
                self.port = a.port();
                self.v6 = true;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PacketData {
    pub packets: Vec<Packet>,
}

pub type SharedPacketData = Arc<RwLock<PacketData>>;
pub type Recycler = Arc<Mutex<Vec<SharedPacketData>>>;
pub type Receiver = mpsc::Receiver<SharedPacketData>;
pub type Sender = mpsc::Sender<SharedPacketData>;

impl PacketData {
    pub fn new() -> PacketData {
        PacketData {
            packets: vec![Packet::default(); BLOCK_SIZE],
        }
    }
    fn run_read_from(&mut self, socket: &UdpSocket) -> Result<usize> {
        self.packets.resize(BLOCK_SIZE, Packet::default());
        let mut i = 0;
        for p in self.packets.iter_mut() {
            p.size = 0;
            match socket.recv_from(&mut p.data) {
                Err(_) if i > 0 => {
                    trace!("got {:?} messages", i);
                    break;
                }
                Err(e) => {
                    info!("recv_from err {:?}", e);
                    return Err(Error::IO(e));
                }
                Ok((nrecv, from)) => {
                    p.size = nrecv;
                    p.set_addr(&from);
                    if i == 0 {
                        socket.set_nonblocking(true)?;
                    }
                }
            }
            i += 1;
        }
        Ok(i)
    }
    fn read_from(&mut self, socket: &UdpSocket) -> Result<()> {
        let sz = self.run_read_from(socket)?;
        self.packets.resize(sz, Packet::default());
        Ok(())
    }
    fn send_to(&self, socket: &UdpSocket, num: &mut usize) -> Result<()> {
        for p in self.packets.iter() {
            let a = p.get_addr();
            socket.send_to(&p.data[0..p.size], &a)?;
            //TODO(anatoly): wtf do we do about errors?
            *num += 1;
        }
        Ok(())
    }
}

pub fn allocate(recycler: Recycler) -> SharedPacketData {
    let mut gc = recycler.lock().expect("lock");
    gc.pop()
        .unwrap_or_else(|| Arc::new(RwLock::new(PacketData::new())))
}

pub fn recycle(recycler: Recycler, msgs: SharedPacketData) {
    let mut gc = recycler.lock().expect("lock");
    gc.push(msgs);
}

fn recv_loop(
    sock: &UdpSocket,
    exit: Arc<Mutex<bool>>,
    recycler: Recycler,
    channel: Sender,
) -> Result<()> {
    loop {
        let msgs = allocate(recycler.clone());
        let msgs_ = msgs.clone();
        loop {
            match msgs.write().unwrap().read_from(&sock) {
                Ok(()) => {
                    channel.send(msgs_)?;
                    break;
                }
                Err(_) => {
                    if *exit.lock().unwrap() {
                        recycle(recycler.clone(), msgs_);
                        return Ok(());
                    }
                }
            }
        }
    }
}

pub fn receiver(
    sock: UdpSocket,
    exit: Arc<Mutex<bool>>,
    recycler: Recycler,
    channel: Sender,
) -> Result<JoinHandle<()>> {
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))?;
    Ok(spawn(move || {
        let _ = recv_loop(&sock, exit, recycler, channel);
        ()
    }))
}

fn recv_send(sock: &UdpSocket, recycler: Recycler, r: &Receiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    let msgs_ = msgs.clone();
    let mut num = 0;
    msgs.read().unwrap().send_to(sock, &mut num)?;
    recycle(recycler, msgs_);
    Ok(())
}

pub fn sender(
    sock: UdpSocket,
    exit: Arc<Mutex<bool>>,
    recycler: Recycler,
    r: Receiver,
) -> JoinHandle<()> {
    spawn(move || loop {
        if recv_send(&sock, recycler.clone(), &r).is_err() && *exit.lock().unwrap() {
            break;
        }
    })
}

#[cfg(test)]
mod test {
    use std::thread::sleep;
    use std::sync::{Arc, Mutex};
    use std::net::{SocketAddr, UdpSocket};
    use std::time::Duration;
    use std::time::SystemTime;
    use std::thread::{spawn, JoinHandle};
    use std::sync::mpsc::channel;
    use result::Result;
    use streamer::{allocate, receiver, recycle, sender, Packet, Receiver, Recycler, PACKET_SIZE};

    fn producer(addr: &SocketAddr, recycler: Recycler, exit: Arc<Mutex<bool>>) -> JoinHandle<()> {
        let send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let msgs = allocate(recycler.clone());
        msgs.write().unwrap().packets.resize(10, Packet::default());
        for w in msgs.write().unwrap().packets.iter_mut() {
            w.size = PACKET_SIZE;
            w.set_addr(&addr);
        }
        spawn(move || loop {
            if *exit.lock().unwrap() {
                return;
            }
            let mut num = 0;
            msgs.read().unwrap().send_to(&send, &mut num).unwrap();
            assert_eq!(num, 10);
        })
    }

    fn sinc(
        recycler: Recycler,
        exit: Arc<Mutex<bool>>,
        rvs: Arc<Mutex<usize>>,
        r: Receiver,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            if *exit.lock().unwrap() {
                return;
            }
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(msgs) => {
                    let msgs_ = msgs.clone();
                    *rvs.lock().unwrap() += msgs.read().unwrap().packets.len();
                    recycle(recycler.clone(), msgs_);
                }
                _ => (),
            }
        })
    }
    fn run_streamer_bench() -> Result<()> {
        let read = UdpSocket::bind("127.0.0.1:0")?;
        let addr = read.local_addr()?;
        let exit = Arc::new(Mutex::new(false));
        let recycler = Arc::new(Mutex::new(Vec::new()));

        let (s_reader, r_reader) = channel();
        let t_reader = receiver(read, exit.clone(), recycler.clone(), s_reader)?;
        let t_producer1 = producer(&addr, recycler.clone(), exit.clone());
        let t_producer2 = producer(&addr, recycler.clone(), exit.clone());
        let t_producer3 = producer(&addr, recycler.clone(), exit.clone());

        let rvs = Arc::new(Mutex::new(0));
        let t_sinc = sinc(recycler.clone(), exit.clone(), rvs.clone(), r_reader);

        let start = SystemTime::now();
        let start_val = *rvs.lock().unwrap();
        sleep(Duration::new(5, 0));
        let elapsed = start.elapsed().unwrap();
        let end_val = *rvs.lock().unwrap();
        let time = elapsed.as_secs() * 10000000000 + elapsed.subsec_nanos() as u64;
        let ftime = (time as f64) / 10000000000f64;
        let fcount = (end_val - start_val) as f64;
        println!("performance: {:?}", fcount / ftime);
        *exit.lock().unwrap() = true;
        t_reader.join()?;
        t_producer1.join()?;
        t_producer2.join()?;
        t_producer3.join()?;
        t_sinc.join()?;
        Ok(())
    }
    #[test]
    pub fn streamer_bench() {
        run_streamer_bench().unwrap();
    }

    fn get_msgs(r: Receiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.read().unwrap().packets.len(),
                e => println!("error {:?}", e),
            }
            if *num == 10 {
                break;
            }
        }
    }
    #[cfg(ipv6)]
    #[test]
    pub fn streamer_send_test_ipv6() {
        let read = UdpSocket::bind("[::1]:0").expect("bind");
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("[::1]:0").expect("bind");
        let exit = Arc::new(Mutex::new(false));
        let recycler = Arc::new(Mutex::new(Vec::new()));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), recycler.clone(), s_reader).unwrap();
        let (s_sender, r_sender) = channel();
        let t_sender = sender(send, exit.clone(), recycler.clone(), r_sender);
        let msgs = allocate(recycler.clone());
        msgs.write().unwrap().packets.resize(10, Packet::default());
        for (i, w) in msgs.write().unwrap().packets.iter_mut().enumerate() {
            w.data[0] = i as u8;
            w.size = PACKET_SIZE;
            w.set_addr(&addr);
            assert_eq!(w.get_addr(), addr);
        }
        s_sender.send(msgs).expect("send");
        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        *exit.lock().unwrap() = true;
        t_receiver.join().expect("join");
        t_sender.join().expect("join");
    }

    #[test]
    pub fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(Mutex::new(false));
        let recycler = Arc::new(Mutex::new(Vec::new()));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), recycler.clone(), s_reader).unwrap();
        let (s_sender, r_sender) = channel();
        let t_sender = sender(send, exit.clone(), recycler.clone(), r_sender);
        let msgs = allocate(recycler.clone());
        msgs.write().unwrap().packets.resize(10, Packet::default());
        for (i, w) in msgs.write().unwrap().packets.iter_mut().enumerate() {
            w.data[0] = i as u8;
            w.size = PACKET_SIZE;
            w.set_addr(&addr);
            assert_eq!(w.get_addr(), addr);
        }
        s_sender.send(msgs).expect("send");
        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        *exit.lock().unwrap() = true;
        t_receiver.join().expect("join");
        t_sender.join().expect("join");
    }
}
