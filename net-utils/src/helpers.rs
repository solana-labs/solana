#![allow(clippy::integer_arithmetic)]
use crate::ip_echo_server::IpEchoServerMessage;
use crate::socket_like::*;
use crate::{DatagramSocket, MAX_PORT_COUNT_PER_MESSAGE};
use {
    log::*,
    std::{
        collections::{BTreeMap, HashSet},
        io::{self, Read, Write},
        net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
        sync::{mpsc::channel, Arc, RwLock},
        time::{Duration, Instant},
    },
    url::Url,
};

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

pub fn parse_port_range(port_range: &str) -> Option<crate::PortRange> {
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
    // First, check if the host syntax is valid. This check is needed because addresses
    // such as `("localhost:1234", 0)` will resolve to IPs on some networks.
    let parsed_url = Url::parse(&format!("http://{}", host)).map_err(|e| e.to_string())?;
    if parsed_url.port().is_some() {
        return Err(format!("Expected port in URL: {}", host));
    }

    // Next, check to see if it resolves to an IP address
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
        .map_err(|err| format!("Unable to resolve host {}: {}", host_port, err))?
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

pub(crate) const HEADER_LENGTH: usize = 4;
pub(crate) fn ip_echo_server_reply_length() -> usize {
    let largest_ip_addr = IpAddr::from([0u16; 8]); // IPv6 variant
    HEADER_LENGTH + bincode::serialized_size(&largest_ip_addr).unwrap() as usize
}

fn ip_echo_server_request(
    ip_echo_server_addr: &SocketAddr,
    msg: crate::IpEchoServerMessage,
) -> Result<IpAddr, String> {
    let mut data = vec![0u8; ip_echo_server_reply_length()];

    let timeout = Duration::new(5, 0);
    TcpStream::connect_timeout(ip_echo_server_addr, timeout)
        .and_then(|mut stream| {
            // Start with HEADER_LENGTH null bytes to avoid looking like an HTTP GET/POST request
            let mut bytes = vec![0; HEADER_LENGTH];

            bytes.append(&mut bincode::serialize(&msg).expect("serialize IpEchoServerMessage"));

            // End with '\n' to make this request look HTTP-ish and tickle an error response back
            // from an HTTP server
            bytes.push(b'\n');

            stream.set_read_timeout(Some(Duration::new(10, 0)))?;
            stream.write_all(&bytes)?;
            stream.shutdown(std::net::Shutdown::Write)?;
            stream.read(data.as_mut_slice())
        })
        .and_then(|_| {
            // It's common for users to accidentally confuse the validator's gossip port and JSON
            // RPC port.  Attempt to detect when this occurs by looking for the standard HTTP
            // response header and provide the user with a helpful error message
            if data.len() < HEADER_LENGTH {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Response too short, received {} bytes", data.len()),
                ));
            }

            let response_header: String =
                data[0..HEADER_LENGTH].iter().map(|b| *b as char).collect();
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

            bincode::deserialize(&data[HEADER_LENGTH..]).map_err(|err| {
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

// Checks if any of the provided TCP/UDP ports are not reachable by the machine at
// `ip_echo_server_addr`
const DEFAULT_TIMEOUT_SECS: u64 = 5;
const DEFAULT_RETRY_COUNT: usize = 5;

pub(crate) fn do_verify_reachable_ports(
    ip_echo_server_addr: &SocketAddr,
    tcp_listeners: Vec<(u16, TcpListener)>,
    udp_sockets: &[&DatagramSocket],
    timeout: u64,
    udp_retry_count: usize,
) -> bool {
    info!(
        "Checking that tcp ports {:?} are reachable from {:?}",
        tcp_listeners, ip_echo_server_addr
    );

    let tcp_ports: Vec<_> = tcp_listeners.iter().map(|(port, _)| *port).collect();
    let _ = ip_echo_server_request(
        ip_echo_server_addr,
        IpEchoServerMessage::new(&tcp_ports, &[]),
    )
    .map_err(|err| warn!("ip_echo_server request failed: {}", err));

    let mut ok = true;
    let timeout = Duration::from_secs(timeout);

    // Wait for a connection to open on each TCP port
    for (port, tcp_listener) in tcp_listeners {
        let (sender, receiver) = channel();
        let listening_addr = tcp_listener.local_addr().unwrap();
        let thread_handle = std::thread::spawn(move || {
            debug!("Waiting for incoming connection on tcp/{}", port);
            match tcp_listener.incoming().next() {
                Some(_) => sender
                    .send(())
                    .unwrap_or_else(|err| warn!("send failure: {}", err)),
                None => warn!("tcp incoming failed"),
            }
        });
        match receiver.recv_timeout(timeout) {
            Ok(_) => {
                info!("tcp/{} is reachable", port);
            }
            Err(err) => {
                error!(
                    "Received no response at tcp/{}, check your port configuration: {}",
                    port, err
                );
                // Ugh, std rustc doesn't provide acceptng with timeout or restoring original
                // nonblocking-status of sockets because of lack of getter, only the setter...
                // So, to close the thread cleanly, just connect from here.
                // ref: https://github.com/rust-lang/rust/issues/31615
                TcpStream::connect_timeout(&listening_addr, timeout).unwrap();
                ok = false;
            }
        }
        // ensure to reap the thread
        thread_handle.join().unwrap();
    }

    if !ok {
        // No retries for TCP, abort on the first failure
        return ok;
    }

    let mut udp_ports: BTreeMap<_, _> = BTreeMap::new();
    udp_sockets.iter().for_each(|udp_socket| {
        let port = udp_socket.local_addr().unwrap().port();
        udp_ports
            .entry(port)
            .or_insert_with(Vec::new)
            .push(udp_socket);
    });
    let udp_ports: Vec<_> = udp_ports.into_iter().collect();

    info!(
        "Checking that udp ports {:?} are reachable from {:?}",
        udp_ports.iter().map(|(port, _)| port).collect::<Vec<_>>(),
        ip_echo_server_addr
    );

    'outer: for checked_ports_and_sockets in udp_ports.chunks(MAX_PORT_COUNT_PER_MESSAGE) {
        ok = false;

        for udp_remaining_retry in (0_usize..udp_retry_count).rev() {
            let (checked_ports, checked_socket_iter) = (
                checked_ports_and_sockets
                    .iter()
                    .map(|(port, _)| *port)
                    .collect::<Vec<_>>(),
                checked_ports_and_sockets
                    .iter()
                    .flat_map(|(_, sockets)| sockets),
            );

            let _ = ip_echo_server_request(
                ip_echo_server_addr,
                IpEchoServerMessage::new(&[], &checked_ports),
            )
            .map_err(|err| warn!("ip_echo_server request failed: {}", err));

            // Spawn threads at once!
            let reachable_ports = Arc::new(RwLock::new(HashSet::new()));
            let thread_handles: Vec<_> = checked_socket_iter
                .map(|udp_socket| {
                    let port = udp_socket.local_addr().unwrap().port();
                    let udp_socket = udp_socket.try_clone().expect("Unable to clone udp socket");
                    let reachable_ports = reachable_ports.clone();
                    std::thread::spawn(move || {
                        let start = Instant::now();

                        let original_read_timeout = udp_socket.read_timeout().unwrap();
                        udp_socket
                            .set_read_timeout(Some(Duration::from_millis(250)))
                            .unwrap();
                        loop {
                            if reachable_ports.read().unwrap().contains(&port)
                                || Instant::now().duration_since(start) >= timeout
                            {
                                break;
                            }

                            let recv_result = udp_socket.recv(&mut [0; 1]);
                            debug!(
                                "Waited for incoming datagram on udp/{}: {:?}",
                                port, recv_result
                            );

                            if recv_result.is_ok() {
                                reachable_ports.write().unwrap().insert(port);
                                break;
                            }
                        }
                        udp_socket.set_read_timeout(original_read_timeout).unwrap();
                    })
                })
                .collect();

            // Now join threads!
            // Separate from the above by collect()-ing as an intermediately step to make the iterator
            // eager not lazy so that joining happens here at once after creating bunch of threads
            // at once.
            for thread in thread_handles {
                thread.join().unwrap();
            }

            let reachable_ports = reachable_ports.read().unwrap().clone();
            if reachable_ports.len() == checked_ports.len() {
                info!(
                    "checked udp ports: {:?}, reachable udp ports: {:?}",
                    checked_ports, reachable_ports
                );
                ok = true;
                break;
            } else if udp_remaining_retry > 0 {
                // Might have lost a UDP packet, retry a couple times
                error!(
                    "checked udp ports: {:?}, reachable udp ports: {:?}",
                    checked_ports, reachable_ports
                );
                error!("There are some udp ports with no response!! Retrying...");
            } else {
                error!("Maximum retry count is reached....");
                break 'outer;
            }
        }
    }

    ok
}

pub fn verify_reachable_ports(
    ip_echo_server_addr: &SocketAddr,
    tcp_listeners: Vec<(u16, TcpListener)>,
    udp_sockets: &[&DatagramSocket],
) -> bool {
    do_verify_reachable_ports(
        ip_echo_server_addr,
        tcp_listeners,
        udp_sockets,
        DEFAULT_TIMEOUT_SECS,
        DEFAULT_RETRY_COUNT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
