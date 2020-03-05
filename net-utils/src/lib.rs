//! The `net_utils` module assists with networking
use log::*;
use rand::{thread_rng, Rng};
use socket2::{Domain, SockAddr, Socket, Type};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::channel;
use std::time::Duration;

mod ip_echo_server;
use ip_echo_server::IpEchoServerMessage;
pub use ip_echo_server::{ip_echo_server, IpEchoServer};

/// A data type representing a public Udp socket
pub struct UdpSocketPair {
    pub addr: SocketAddr,    // Public address of the socket
    pub receiver: UdpSocket, // Locally bound socket that can receive from the public address
    pub sender: UdpSocket,   // Locally bound socket to send via public address
}

pub type PortRange = (u16, u16);

fn ip_echo_server_request(
    ip_echo_server_addr: &SocketAddr,
    msg: IpEchoServerMessage,
) -> Result<IpAddr, String> {
    let mut data = Vec::new();

    let timeout = Duration::new(5, 0);
    TcpStream::connect_timeout(ip_echo_server_addr, timeout)
        .and_then(|mut stream| {
            let mut bytes = vec![0; 4]; // Start with 4 null bytes to avoid looking like an HTTP GET/POST request

            bytes.append(&mut bincode::serialize(&msg).expect("serialize IpEchoServerMessage"));

            // End with '\n' to make this request look HTTP-ish and tickle an error response back
            // from an HTTP server
            bytes.push(b'\n');

            stream.set_read_timeout(Some(Duration::new(10, 0)))?;
            stream.write_all(&bytes)?;
            stream.shutdown(std::net::Shutdown::Write)?;
            stream.read_to_end(&mut data)
        })
        .and_then(|_| {
            // It's common for users to accidentally confuse the validator's gossip port and JSON
            // RPC port.  Attempt to detect when this occurs by looking for the standard HTTP
            // response header and provide the user with a helpful error message
            if data.len() < 4 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Response too short, received {} bytes", data.len()),
                ));
            }

            let response_header: String = data[0..4].iter().map(|b| *b as char).collect();
            if response_header != "\0\0\0\0" {
                if response_header == "HTTP" {
                    let http_response = data.iter().map(|b| *b as char).collect::<String>();
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Invalid gossip entrypoint. {} looks to be an HTTP port: {}",
                            ip_echo_server_addr, http_response
                        ),
                    ));
                }
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Invalid gossip entrypoint. {} provided an invalid response header: '{}'",
                        ip_echo_server_addr, response_header
                    ),
                ));
            }

            bincode::deserialize(&data[4..]).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to deserialize: {:?}", err),
                )
            })
        })
        .map_err(|err| err.to_string())
}

/// Determine the public IP address of this machine by asking an ip_echo_server at the given
/// address
pub fn get_public_ip_addr(ip_echo_server_addr: &SocketAddr) -> Result<IpAddr, String> {
    ip_echo_server_request(ip_echo_server_addr, IpEchoServerMessage::default())
}

// Aborts the process if any of the provided TCP/UDP ports are not reachable by the machine at
// `ip_echo_server_addr`
pub fn verify_reachable_ports(
    ip_echo_server_addr: &SocketAddr,
    tcp_listeners: Vec<(u16, TcpListener)>,
    udp_sockets: &[&UdpSocket],
) {
    let udp: Vec<(_, _)> = udp_sockets
        .iter()
        .map(|udp_socket| {
            (
                udp_socket.local_addr().unwrap().port(),
                udp_socket.try_clone().expect("Unable to clone udp socket"),
            )
        })
        .collect();

    let udp_ports: Vec<_> = udp.iter().map(|x| x.0).collect();

    info!(
        "Checking that tcp ports {:?} and udp ports {:?} are reachable from {:?}",
        tcp_listeners, udp_ports, ip_echo_server_addr
    );

    let tcp_ports: Vec<_> = tcp_listeners.iter().map(|(port, _)| *port).collect();
    let _ = ip_echo_server_request(
        ip_echo_server_addr,
        IpEchoServerMessage::new(&tcp_ports, &udp_ports),
    )
    .map_err(|err| warn!("ip_echo_server request failed: {}", err));

    // Wait for a connection to open on each TCP port
    for (port, tcp_listener) in tcp_listeners {
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            debug!("Waiting for incoming connection on tcp/{}", port);
            let _ = tcp_listener.incoming().next().expect("tcp incoming failed");
            sender.send(()).expect("send failure");
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|err| {
                error!(
                    "Received no response at tcp/{}, check your port configuration: {}",
                    port, err
                );
                std::process::exit(1);
            });
        info!("tcp/{} is reachable", port);
    }

    // Wait for a datagram to arrive at each UDP port
    for (port, udp_socket) in udp {
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut buf = [0; 1];
            debug!("Waiting for incoming datagram on udp/{}", port);
            let _ = udp_socket.recv(&mut buf).expect("udp recv failure");
            sender.send(()).expect("send failure");
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|err| {
                error!(
                    "Received no response at udp/{}, check your port configuration: {}",
                    port, err
                );
                std::process::exit(1);
            });
        info!("udp/{} is reachable", port);
    }
}

pub fn parse_port_or_addr(optstr: Option<&str>, default_addr: SocketAddr) -> SocketAddr {
    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = default_addr;
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            default_addr
        }
    } else {
        default_addr
    }
}

pub fn parse_port_range(port_range: &str) -> Option<PortRange> {
    let ports: Vec<&str> = port_range.split('-').collect();
    if ports.len() != 2 {
        return None;
    }

    let start_port = ports[0].parse();
    let end_port = ports[1].parse();

    if start_port.is_err() || end_port.is_err() {
        return None;
    }
    let start_port = start_port.unwrap();
    let end_port = end_port.unwrap();
    if end_port < start_port {
        return None;
    }
    Some((start_port, end_port))
}

pub fn parse_host(host: &str) -> Result<IpAddr, String> {
    let ips: Vec<_> = (host, 0)
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .map(|socket_address| socket_address.ip())
        .collect();
    if ips.is_empty() {
        Err(format!("Unable to resolve host: {}", host))
    } else {
        Ok(ips[0])
    }
}

pub fn is_host(string: String) -> Result<(), String> {
    parse_host(&string).map(|_| ())
}

pub fn parse_host_port(host_port: &str) -> Result<SocketAddr, String> {
    let addrs: Vec<_> = host_port
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .collect();
    if addrs.is_empty() {
        Err(format!("Unable to resolve host: {}", host_port))
    } else {
        Ok(addrs[0])
    }
}

pub fn is_host_port(string: String) -> Result<(), String> {
    parse_host_port(&string).map(|_| ())
}

#[cfg(windows)]
fn udp_socket(_reuseaddr: bool) -> io::Result<Socket> {
    let sock = Socket::new(Domain::ipv4(), Type::dgram(), None)?;
    Ok(sock)
}

#[cfg(not(windows))]
fn udp_socket(reuseaddr: bool) -> io::Result<Socket> {
    use nix::sys::socket::setsockopt;
    use nix::sys::socket::sockopt::{ReuseAddr, ReusePort};
    use std::os::unix::io::AsRawFd;

    let sock = Socket::new(Domain::ipv4(), Type::dgram(), None)?;
    let sock_fd = sock.as_raw_fd();

    if reuseaddr {
        // best effort, i.e. ignore errors here, we'll get the failure in caller
        setsockopt(sock_fd, ReusePort, &true).ok();
        setsockopt(sock_fd, ReuseAddr, &true).ok();
    }

    Ok(sock)
}

// Find a port in the given range that is available for both TCP and UDP
pub fn bind_common_in_range(
    ip_addr: IpAddr,
    range: PortRange,
) -> io::Result<(u16, (UdpSocket, TcpListener))> {
    let (start, end) = range;
    let mut tries_left = end - start;
    let mut rand_port = thread_rng().gen_range(start, end);
    loop {
        match bind_common(ip_addr, rand_port, false) {
            Ok((sock, listener)) => {
                break Result::Ok((sock.local_addr().unwrap().port(), (sock, listener)));
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

pub fn bind_in_range(ip_addr: IpAddr, range: PortRange) -> io::Result<(u16, UdpSocket)> {
    let sock = udp_socket(false)?;

    let (start, end) = range;
    let mut tries_left = end - start;
    let mut rand_port = thread_rng().gen_range(start, end);
    loop {
        let addr = SocketAddr::new(ip_addr, rand_port);

        match sock.bind(&SockAddr::from(addr)) {
            Ok(_) => {
                let sock = sock.into_udp_socket();
                break Result::Ok((sock.local_addr().unwrap().port(), sock));
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

// binds many sockets to the same port in a range
pub fn multi_bind_in_range(
    ip_addr: IpAddr,
    range: PortRange,
    mut num: usize,
) -> io::Result<(u16, Vec<UdpSocket>)> {
    if cfg!(windows) && num != 1 {
        // See https://github.com/solana-labs/solana/issues/4607
        warn!(
            "multi_bind_in_range() only supports 1 socket in windows ({} requested)",
            num
        );
        num = 1;
    }
    let mut sockets = Vec::with_capacity(num);

    let port = {
        let (port, _) = bind_in_range(ip_addr, range)?;
        port
    }; // drop the probe, port should be available... briefly.

    for _ in 0..num {
        sockets.push(bind_to(ip_addr, port, true)?);
    }
    Ok((port, sockets))
}

pub fn bind_to(ip_addr: IpAddr, port: u16, reuseaddr: bool) -> io::Result<UdpSocket> {
    let sock = udp_socket(reuseaddr)?;

    let addr = SocketAddr::new(ip_addr, port);

    sock.bind(&SockAddr::from(addr))
        .and_then(|_| Result::Ok(sock.into_udp_socket()))
}

// binds both a UdpSocket and a TcpListener
pub fn bind_common(
    ip_addr: IpAddr,
    port: u16,
    reuseaddr: bool,
) -> io::Result<(UdpSocket, TcpListener)> {
    let sock = udp_socket(reuseaddr)?;

    let addr = SocketAddr::new(ip_addr, port);
    let sock_addr = SockAddr::from(addr);
    sock.bind(&sock_addr).and_then(|_| {
        TcpListener::bind(&addr).and_then(|listener| Result::Ok((sock.into_udp_socket(), listener)))
    })
}

pub fn find_available_port_in_range(ip_addr: IpAddr, range: PortRange) -> io::Result<u16> {
    let (start, end) = range;
    let mut tries_left = end - start;
    let mut rand_port = thread_rng().gen_range(start, end);
    loop {
        match bind_common(ip_addr, rand_port, false) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_parse_port_or_addr() {
        let p1 = parse_port_or_addr(Some("9000"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p1.port(), 9000);
        let p2 = parse_port_or_addr(Some("127.0.0.1:7000"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p2.port(), 7000);
        let p2 = parse_port_or_addr(Some("hi there"), SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p2.port(), 1);
        let p3 = parse_port_or_addr(None, SocketAddr::from(([1, 2, 3, 4], 1)));
        assert_eq!(p3.port(), 1);
    }

    #[test]
    fn test_parse_port_range() {
        assert_eq!(parse_port_range("garbage"), None);
        assert_eq!(parse_port_range("1-"), None);
        assert_eq!(parse_port_range("1-2"), Some((1, 2)));
        assert_eq!(parse_port_range("1-2-3"), None);
        assert_eq!(parse_port_range("2-1"), None);
    }

    #[test]
    fn test_parse_host() {
        parse_host("localhost:1234").unwrap_err();
        parse_host("localhost").unwrap();
        parse_host("127.0.0.0:1234").unwrap_err();
        parse_host("127.0.0.0").unwrap();
    }

    #[test]
    fn test_parse_host_port() {
        parse_host_port("localhost:1234").unwrap();
        parse_host_port("localhost").unwrap_err();
        parse_host_port("127.0.0.0:1234").unwrap();
        parse_host_port("127.0.0.0").unwrap_err();
    }

    #[test]
    fn test_is_host_port() {
        assert!(is_host_port("localhost:1234".to_string()).is_ok());
        assert!(is_host_port("localhost".to_string()).is_err());
    }

    #[test]
    fn test_bind() {
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(bind_in_range(ip_addr, (2000, 2001)).unwrap().0, 2000);
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let x = bind_to(ip_addr, 2002, true).unwrap();
        let y = bind_to(ip_addr, 2002, true).unwrap();
        assert_eq!(
            x.local_addr().unwrap().port(),
            y.local_addr().unwrap().port()
        );
        bind_to(ip_addr, 2002, false).unwrap_err();
        bind_in_range(ip_addr, (2002, 2003)).unwrap_err();

        let (port, v) = multi_bind_in_range(ip_addr, (2010, 2110), 10).unwrap();
        for sock in &v {
            assert_eq!(port, sock.local_addr().unwrap().port());
        }
    }

    #[test]
    #[should_panic]
    fn test_bind_in_range_nil() {
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let _ = bind_in_range(ip_addr, (2000, 2000));
    }

    #[test]
    fn test_find_available_port_in_range() {
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(
            find_available_port_in_range(ip_addr, (3000, 3001)).unwrap(),
            3000
        );
        let port = find_available_port_in_range(ip_addr, (3000, 3050)).unwrap();
        assert!(3000 <= port && port < 3050);

        let _socket = bind_to(ip_addr, port, false).unwrap();
        find_available_port_in_range(ip_addr, (port, port + 1)).unwrap_err();
    }

    #[test]
    fn test_bind_common_in_range() {
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (port, _sockets) = bind_common_in_range(ip_addr, (3100, 3150)).unwrap();
        assert!(3100 <= port && port < 3150);

        bind_common_in_range(ip_addr, (port, port + 1)).unwrap_err();
    }

    #[test]
    fn test_get_public_ip_addr() {
        solana_logger::setup();
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let (_server_port, (server_udp_socket, server_tcp_listener)) =
            bind_common_in_range(ip_addr, (3200, 3250)).unwrap();
        let (client_port, (client_udp_socket, client_tcp_listener)) =
            bind_common_in_range(ip_addr, (3200, 3250)).unwrap();

        let _runtime = ip_echo_server(server_tcp_listener);

        let ip_echo_server_addr = server_udp_socket.local_addr().unwrap();
        assert_eq!(
            get_public_ip_addr(&ip_echo_server_addr),
            parse_host("127.0.0.1"),
        );

        verify_reachable_ports(
            &ip_echo_server_addr,
            vec![(client_port, client_tcp_listener)],
            &[&client_udp_socket],
        );
    }
}
