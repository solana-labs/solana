//! The 'streamer` module allows for efficient batch processing of UDP packets.

use result::{Error, Result};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{spawn, JoinHandle};
use std::time::Duration;

const BLOCK_SIZE: usize = 1024 * 8;
pub const PACKET_SIZE: usize = 256;
pub const RESP_SIZE: usize = 64 * 1024;
pub const NUM_RESP: usize = (BLOCK_SIZE * PACKET_SIZE) / RESP_SIZE;

#[derive(Clone, Default)]
pub struct Meta {
    pub size: usize,
    pub addr: [u16; 8],
    pub port: u16,
    pub v6: bool,
}

#[derive(Clone)]
pub struct Packet {
    pub data: [u8; PACKET_SIZE],
    pub meta: Meta,
}

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Packet {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.meta.get_addr()
        )
    }
}

impl Default for Packet {
    fn default() -> Packet {
        Packet {
            data: [0u8; PACKET_SIZE],
            meta: Meta::default(),
        }
    }
}

impl Meta {
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

impl Default for Packets {
    fn default() -> Packets {
        Packets {
            packets: vec![Packet::default(); BLOCK_SIZE],
        }
    }
}

#[derive(Clone)]
pub struct Response {
    pub data: [u8; RESP_SIZE],
    pub meta: Meta,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Response {{ size: {:?}, addr: {:?} }}",
            self.meta.size,
            self.meta.get_addr()
        )
    }
}

impl Default for Response {
    fn default() -> Response {
        Response {
            data: [0u8; RESP_SIZE],
            meta: Meta::default(),
        }
    }
}

#[derive(Debug)]
pub struct Responses {
    pub responses: Vec<Response>,
}

impl Default for Responses {
    fn default() -> Responses {
        Responses {
            responses: vec![Response::default(); NUM_RESP],
        }
    }
}

pub type Receiver = mpsc::Receiver<Packets>;
pub type Sender = mpsc::Sender<Packets>;
pub type Responder = mpsc::Sender<Responses>;
pub type ResponseReceiver = mpsc::Receiver<Responses>;

impl Packets {
    fn run_read_from(&mut self, socket: &UdpSocket) -> Result<usize> {
        self.packets.resize(BLOCK_SIZE, Packet::default());
        let mut i = 0;
        socket.set_nonblocking(false)?;
        for p in &mut self.packets {
            p.meta.size = 0;
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
    pub fn read_from(&mut self, socket: &UdpSocket) -> Result<()> {
        let sz = self.run_read_from(socket)?;
        self.packets.resize(sz, Packet::default());
        Ok(())
    }
}

impl Responses {
    pub fn send_to(&self, socket: &UdpSocket, num: &mut usize) -> Result<()> {
        for p in &self.responses {
            let a = p.meta.get_addr();
            socket.send_to(&p.data[..p.meta.size], &a)?;
            //TODO(anatoly): wtf do we do about errors?
            *num += 1;
        }
        Ok(())
    }
}

fn recv_loop(sock: &UdpSocket, exit: &Arc<AtomicBool>, channel: &Sender) -> Result<()> {
    loop {
        let mut msgs = Packets::default();
        loop {
            let ret = msgs.read_from(sock);
            match ret {
                Ok(()) => {
                    channel.send(msgs)?;
                    break;
                }
                Err(_) => {
                    if exit.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                }
            }
        }
    }
}

pub fn receiver(sock: UdpSocket, exit: Arc<AtomicBool>, channel: Sender) -> Result<JoinHandle<()>> {
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))?;
    Ok(spawn(move || {
        let _ = recv_loop(&sock, &exit, &channel);
        ()
    }))
}

fn recv_send(sock: &UdpSocket, r: &ResponseReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    let mut num = 0;
    msgs.send_to(sock, &mut num)?;
    Ok(())
}

pub fn responder(sock: UdpSocket, exit: Arc<AtomicBool>, r: ResponseReceiver) -> JoinHandle<()> {
    spawn(move || loop {
        if recv_send(&sock, &r).is_err() && exit.load(Ordering::Relaxed) {
            break;
        }
    })
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use result::Result;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::thread::{spawn, JoinHandle};
    use std::time::Duration;
    use std::time::SystemTime;
    use streamer::{receiver, Packet, Packets, Receiver, PACKET_SIZE};

    fn producer(addr: &SocketAddr, exit: Arc<AtomicBool>) -> JoinHandle<()> {
        let send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut msgs = Packets::default();
        msgs.packets.resize(10, Packet::default());
        for w in &mut msgs.packets {
            w.meta.size = PACKET_SIZE;
            w.meta.set_addr(&addr);
        }
        spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                return;
            }
            let mut num = 0;
            for p in &msgs.packets {
                let a = p.meta.get_addr();
                send.send_to(&p.data[..p.meta.size], &a).unwrap();
                num += 1;
            }
            assert_eq!(num, 10);
        })
    }

    fn sink(exit: Arc<AtomicBool>, rvs: Arc<Mutex<usize>>, r: Receiver) -> JoinHandle<()> {
        spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                return;
            }
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(msgs) => {
                    *rvs.lock().unwrap() += msgs.packets.len();
                }
                _ => (),
            }
        })
    }
    fn run_streamer_bench() -> Result<()> {
        let read = UdpSocket::bind("127.0.0.1:0")?;
        let addr = read.local_addr()?;
        let exit = Arc::new(AtomicBool::new(false));

        let (s_reader, r_reader) = channel();
        let t_reader = receiver(read, exit.clone(), s_reader)?;
        let t_producer1 = producer(&addr, exit.clone());
        let t_producer2 = producer(&addr, exit.clone());
        let t_producer3 = producer(&addr, exit.clone());

        let rvs = Arc::new(Mutex::new(0));
        let t_sink = sink(exit.clone(), rvs.clone(), r_reader);

        let start = SystemTime::now();
        let start_val = *rvs.lock().unwrap();
        sleep(Duration::new(5, 0));
        let elapsed = start.elapsed().unwrap();
        let end_val = *rvs.lock().unwrap();
        let time = elapsed.as_secs() * 10000000000 + elapsed.subsec_nanos() as u64;
        let ftime = (time as f64) / 10000000000f64;
        let fcount = (end_val - start_val) as f64;
        println!("performance: {:?}", fcount / ftime);
        exit.store(true, Ordering::Relaxed);
        t_reader.join()?;
        t_producer1.join()?;
        t_producer2.join()?;
        t_producer3.join()?;
        t_sink.join()?;
        Ok(())
    }
    #[bench]
    pub fn streamer_bench(_bench: &mut Bencher) {
        run_streamer_bench().unwrap();
    }
}

#[cfg(test)]
mod test {
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;
    use streamer::{receiver, responder, Packet, Packets, Receiver, Response, Responses,
                   PACKET_SIZE};

    fn get_msgs(r: Receiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.packets.len(),
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
        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), s_reader).unwrap();
        let (s_responder, r_responder) = channel();
        let t_responder = responder(send, exit.clone(), r_responder);
        let msgs = Pakcets::default();
        msgs.packets.resize(10, Packet::default());
        for (i, w) in msgs.packets.iter_mut().enumerate() {
            w.data[0] = i as u8;
            w.size = PACKET_SIZE;
            w.set_addr(&addr);
            assert_eq!(w.get_addr(), addr);
        }
        s_responder.send(msgs).expect("send");
        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
    #[test]
    pub fn streamer_debug() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
        write!(io::sink(), "{:?}", Response::default()).unwrap();
        write!(io::sink(), "{:?}", Responses::default()).unwrap();
    }
    #[test]
    pub fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), s_reader).unwrap();
        let (s_responder, r_responder) = channel();
        let t_responder = responder(send, exit.clone(), r_responder);
        let mut msgs = Responses::default();
        msgs.responses.resize(10, Response::default());
        for (i, w) in msgs.responses.iter_mut().enumerate() {
            w.data[0] = i as u8;
            w.meta.size = PACKET_SIZE;
            w.meta.set_addr(&addr);
            assert_eq!(w.meta.get_addr(), addr);
        }
        s_responder.send(msgs).expect("send");
        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
