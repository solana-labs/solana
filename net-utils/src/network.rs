use crate::{std_network::StdNetwork, DatagramSocket, PortRange};
use enum_dispatch::enum_dispatch;
use std::default::Default;
use std::io::Result;
use std::net::{IpAddr, SocketAddr, TcpListener, ToSocketAddrs};

/// NetworkLike abstracts the source of SocketLike things.
#[enum_dispatch(Network)]
pub trait NetworkLike {
    fn bind_common_in_range(
        &self,
        ip_addr: IpAddr,
        range: PortRange,
    ) -> Result<(u16, (DatagramSocket, TcpListener))>;

    fn bind_in_range(&self, ip_addr: IpAddr, range: PortRange) -> Result<(u16, DatagramSocket)>;

    // binds many sockets to the same port in a range
    fn multi_bind_in_range(
        &self,
        ip_addr: IpAddr,
        range: PortRange,
        num: usize,
    ) -> Result<(u16, Vec<DatagramSocket>)>;

    fn bind_to(&self, ip_addr: IpAddr, port: u16, reuseaddr: bool) -> Result<DatagramSocket>;

    fn bind_common(
        &self,
        ip_addr: IpAddr,
        port: u16,
        reuseaddr: bool,
    ) -> Result<(DatagramSocket, TcpListener)>;

    fn find_available_port_in_range(&self, ip_addr: IpAddr, range: PortRange) -> Result<u16>;

    fn bind<A: ToSocketAddrs>(&self, addresses: A) -> Result<DatagramSocket> {
        each_addr(addresses, |sa| self.bind_to(sa.ip(), sa.port(), false))
    }
}

fn each_addr<A: ToSocketAddrs, F, T>(addr: A, mut f: F) -> Result<T>
where
    F: FnMut(&SocketAddr) -> Result<T>,
{
    for addr in addr.to_socket_addrs()? {
        if let Ok(s) = f(&addr) {
            return Ok(s);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "could not resolve to any addresses",
    ))
}

/// Network wraps NetworkLike implementations, using enum_dispatch to delegate the trait
/// methods to the variants.
#[enum_dispatch]
#[derive(Debug)]
pub enum Network {
    StdNetwork,
    // TODO: add Mock network here
}

impl Default for Network {
    fn default() -> Self {
        StdNetwork {}.into()
    }
}
