use crate::streamer::{packet::Packet, recvmmsg, sendmmsg};
use enum_dispatch::enum_dispatch;
use std::io::Result;
use std::net::{SocketAddr, ToSocketAddrs};

/// SocketLike describes things that act like UdpSockets, but also incorporates methods from
/// streamer.
#[enum_dispatch(DatagramSocket)]
pub trait SocketLike {
    fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<()>;
    fn local_addr(&self) -> Result<SocketAddr>;
    fn send(&mut self, buf: &[u8]) -> Result<usize>;
    fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> Result<usize>;
    fn read_timeout(&self) -> Result<Option<std::time::Duration>>;
    fn set_read_timeout(&self, dur: Option<std::time::Duration>) -> Result<()>;
    fn recv(&self, buf: &mut [u8]) -> Result<usize>;
    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)>;
    fn set_nonblocking(&self, nonblocking: bool) -> Result<()>;
    fn try_clone(&self) -> Result<DatagramSocket>;

    // functions from streamer
    fn multicast(&self, packet: &[u8], dests: &[&SocketAddr]) -> Result<usize>;
    fn send_mmsg(&self, packets: &[(&Vec<u8>, &SocketAddr)]) -> Result<usize>;
    fn recv_mmsg(&self, packets: &mut [Packet]) -> Result<(usize, usize)>;
}

/// DatagramSocket wraps SocketLike implementations, using enum_dispatch to delegate the trait
/// methods to the variants.
#[enum_dispatch]
#[derive(Debug)]
pub enum DatagramSocket {
    StdUdpSocket,
    // TODO: add Mock network socket here
}

impl From<socket2::Socket> for DatagramSocket {
    fn from(sock: socket2::Socket) -> Self {
        StdUdpSocket(sock.into_udp_socket()).into()
    }
}

/// StdUdpSocket implements a SocketLike wrapper for UdpSocket
#[derive(Debug)]
pub struct StdUdpSocket(std::net::UdpSocket);

impl From<socket2::Socket> for StdUdpSocket {
    fn from(sock: socket2::Socket) -> Self {
        StdUdpSocket(sock.into_udp_socket())
    }
}

impl SocketLike for StdUdpSocket {
    fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<()> {
        self.0.connect(addr)
    }
    fn local_addr(&self) -> Result<SocketAddr> {
        self.0.local_addr()
    }
    fn send(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.send(buf)
    }
    fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> Result<usize> {
        self.0.send_to(buf, addr)
    }
    fn read_timeout(&self) -> Result<Option<std::time::Duration>> {
        self.0.read_timeout()
    }
    fn set_read_timeout(&self, dur: Option<std::time::Duration>) -> Result<()> {
        self.0.set_read_timeout(dur)
    }
    fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.0.recv(buf)
    }
    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        self.0.recv_from(buf)
    }
    fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        self.0.set_nonblocking(nonblocking)
    }
    fn try_clone(&self) -> Result<DatagramSocket> {
        Ok(StdUdpSocket(self.0.try_clone()?).into())
    }
    fn multicast(&self, packet: &[u8], dests: &[&SocketAddr]) -> Result<usize> {
        sendmmsg::multicast(&self.0, packet, dests)
    }
    fn send_mmsg(&self, packets: &[(&Vec<u8>, &SocketAddr)]) -> Result<usize> {
        sendmmsg::send_mmsg(&self.0, packets)
    }
    fn recv_mmsg(&self, packets: &mut [Packet]) -> Result<(usize, usize)> {
        recvmmsg::recv_mmsg(&self.0, packets)
    }
}
