#![allow(clippy::integer_arithmetic)]
use {
    log::*,
    rand::{thread_rng, Rng},
    socket2::{SockAddr, Type},
    std::{
        io::{self},
        net::{IpAddr, SocketAddr, TcpListener},
    },
};

use crate::socket_like::*;
use crate::{PortRange, NetworkLike};

/// StdNetwork provides a means to get std::net::UdpSocket's wrapped in DatagramSocket.
#[derive(Clone,Debug)]
pub struct StdNetwork{}

impl StdNetwork {
    #[cfg(windows)]
    fn udp_socket(&self, _reuseaddr: bool) -> io::Result<socket2::Socket> {
        let sock = socket2::Socket::new(socket2::Domain::ipv4(), Type::dgram(), None)?;
        Ok(sock)
    }

    #[cfg(not(windows))]
    fn udp_socket(&self, reuseaddr: bool) -> io::Result<socket2::Socket> {
        use nix::sys::socket::setsockopt;
        use nix::sys::socket::sockopt::{ReuseAddr, ReusePort};
        use std::os::unix::io::AsRawFd;

        let sock = socket2::Socket::new(socket2::Domain::ipv4(), Type::dgram(), None)?;
        let sock_fd = sock.as_raw_fd();

        if reuseaddr {
            // best effort, i.e. ignore errors here, we'll get the failure in caller
            setsockopt(sock_fd, ReusePort, &true).ok();
            setsockopt(sock_fd, ReuseAddr, &true).ok();
        }

        Ok(sock)
    }
}

impl NetworkLike for StdNetwork {
    // Find a port in the given range that is available for both TCP and UDP
    fn bind_common_in_range(
        &self,
        ip_addr: IpAddr,
        range: PortRange,
    ) -> io::Result<(u16, (DatagramSocket, TcpListener))> {
        for port in range.0..range.1 {
            if let Ok((sock, listener)) = self.bind_common(ip_addr, port, false) {
                return Result::Ok((sock.local_addr().unwrap().port(), (sock, listener)));
            }
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("No available TCP/UDP ports in {:?}", range),
        ))
    }

    fn bind_in_range(&self, ip_addr: IpAddr, range: PortRange) -> io::Result<(u16, DatagramSocket)> {
        let sock = self.udp_socket(false)?;

        for port in range.0..range.1 {
            let addr = SocketAddr::new(ip_addr, port);

            if sock.bind(&SockAddr::from(addr)).is_ok() {
                let sock: DatagramSocket = sock.into();
                return Result::Ok((sock.local_addr().unwrap().port(), sock));
            }
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("No available UDP ports in {:?}", range),
        ))
    }

    // binds many sockets to the same port in a range
    fn multi_bind_in_range(
        &self,
        ip_addr: IpAddr,
        range: PortRange,
        mut num: usize,
    ) -> io::Result<(u16, Vec<DatagramSocket>)> {
        if cfg!(windows) && num != 1 {
            // See https://github.com/solana-labs/solana/issues/4607
            warn!(
                "multi_bind_in_range() only supports 1 socket in windows ({} requested)",
                num
            );
            num = 1;
        }
        let mut sockets = Vec::with_capacity(num);

        const NUM_TRIES: usize = 100;
        let mut port = 0;
        let mut error = None;
        for _ in 0..NUM_TRIES {
            port = {
                let (port, _) = self.bind_in_range(ip_addr, range)?;
                port
            }; // drop the probe, port should be available... briefly.

            for _ in 0..num {
                let sock = self.bind_to(ip_addr, port, true);
                if let Ok(sock) = sock {
                    sockets.push(sock);
                } else {
                    error = Some(sock);
                    break;
                }
            }
            if sockets.len() == num {
                break;
            } else {
                sockets.clear();
            }
        }
        if sockets.len() != num {
            error.unwrap()?;
        }
        Ok((port, sockets))
    }

    fn bind_to(&self, ip_addr: IpAddr, port: u16, reuseaddr: bool) -> io::Result<DatagramSocket> {
        let sock = self.udp_socket(reuseaddr)?;

        let addr = SocketAddr::new(ip_addr, port);

        sock.bind(&SockAddr::from(addr))
            .map(|_| sock.into())
    }

    // binds both a DatagramSocket and a TcpListener
    fn bind_common(
        &self,
        ip_addr: IpAddr,
        port: u16,
        reuseaddr: bool,
    ) -> io::Result<(DatagramSocket, TcpListener)> {
        let sock = self.udp_socket(reuseaddr)?;

        let addr = SocketAddr::new(ip_addr, port);
        let sock_addr = SockAddr::from(addr);
        sock.bind(&sock_addr)
            .and_then(|_| TcpListener::bind(&addr).map(|listener| (sock.into(), listener)))
    }

    fn find_available_port_in_range(&self, ip_addr: IpAddr, range: PortRange) -> io::Result<u16> {
        let (start, end) = range;
        let mut tries_left = end - start;
        let mut rand_port = thread_rng().gen_range(start, end);
        loop {
            match self.bind_common(ip_addr, rand_port, false) {
                Ok(_) => {
                    break Ok(rand_port);
                }
                Err(err) => {
                    if tries_left == 0 {
                        return Err(err);
                    }
                }
            }
            rand_port += 1;
            if rand_port == end {
                rand_port = start;
            }
            tries_left -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_bind() {
        let network = StdNetwork{};
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(network.bind_in_range(ip_addr, (2000, 2001)).unwrap().0, 2000);
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let x = network.bind_to(ip_addr, 2002, true).unwrap();
        let y = network.bind_to(ip_addr, 2002, true).unwrap();
        assert_eq!(
            x.local_addr().unwrap().port(),
            y.local_addr().unwrap().port()
        );
        network.bind_to(ip_addr, 2002, false).unwrap_err();
        network.bind_in_range(ip_addr, (2002, 2003)).unwrap_err();

        let (port, v) = network.multi_bind_in_range(ip_addr, (2010, 2110), 10).unwrap();
        for sock in &v {
            assert_eq!(port, sock.local_addr().unwrap().port());
        }
    }

    #[test]
    fn test_bind_in_range_nil() {
        let network = StdNetwork{};
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        network.bind_in_range(ip_addr, (2000, 2000)).unwrap_err();
        network.bind_in_range(ip_addr, (2000, 1999)).unwrap_err();
    }

    #[test]
    fn test_find_available_port_in_range() {
        let network = StdNetwork{};
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(
            network.find_available_port_in_range(ip_addr, (3000, 3001)).unwrap(),
            3000
        );
        let port = network.find_available_port_in_range(ip_addr, (3000, 3050)).unwrap();
        assert!((3000..3050).contains(&port));

        let _socket = network.bind_to(ip_addr, port, false).unwrap();
        network.find_available_port_in_range(ip_addr, (port, port + 1)).unwrap_err();
    }

    #[test]
    fn test_bind_common_in_range() {
        let network = StdNetwork{};
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (port, _sockets) = network.bind_common_in_range(ip_addr, (3100, 3150)).unwrap();
        assert!((3100..3150).contains(&port));

        network.bind_common_in_range(ip_addr, (port, port + 1)).unwrap_err();
    }

    #[test]
    fn test_get_public_ip_addr_none() {
        let network = StdNetwork{};
        solana_logger::setup();
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_server_port, (server_udp_socket, server_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        let _runtime = ip_echo_server(server_tcp_listener);

        let server_ip_echo_addr = server_udp_socket.local_addr().unwrap();
        assert_eq!(
            get_public_ip_addr(&server_ip_echo_addr),
            parse_host("127.0.0.1"),
        );

        assert!(verify_reachable_ports(&server_ip_echo_addr, vec![], &[],));
    }

    #[test]
    fn test_get_public_ip_addr_reachable() {
        let network = StdNetwork{};
        solana_logger::setup();
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_server_port, (server_udp_socket, server_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();
        let (client_port, (client_udp_socket, client_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        let _runtime = ip_echo_server(server_tcp_listener);

        let ip_echo_server_addr = server_udp_socket.local_addr().unwrap();
        assert_eq!(
            get_public_ip_addr(&ip_echo_server_addr),
            parse_host("127.0.0.1"),
        );

        assert!(verify_reachable_ports(
            &ip_echo_server_addr,
            vec![(client_port, client_tcp_listener)],
            &[&client_udp_socket],
        ));
    }

    #[test]
    fn test_get_public_ip_addr_tcp_unreachable() {
        let network = StdNetwork{};
        solana_logger::setup();
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_server_port, (server_udp_socket, _server_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        // make the socket unreachable by not running the ip echo server!

        let server_ip_echo_addr = server_udp_socket.local_addr().unwrap();

        let (correct_client_port, (_client_udp_socket, client_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        assert!(!do_verify_reachable_ports(
            &server_ip_echo_addr,
            vec![(correct_client_port, client_tcp_listener)],
            &[],
            2,
            3,
        ));
    }

    #[test]
    fn test_get_public_ip_addr_udp_unreachable() {
        let network = StdNetwork{};
        solana_logger::setup();
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_server_port, (server_udp_socket, _server_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        // make the socket unreachable by not running the ip echo server!

        let server_ip_echo_addr = server_udp_socket.local_addr().unwrap();

        let (_correct_client_port, (client_udp_socket, _client_tcp_listener)) =
            network.bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        assert!(!do_verify_reachable_ports(
            &server_ip_echo_addr,
            vec![],
            &[&client_udp_socket],
            2,
            3,
        ));
    }
}
