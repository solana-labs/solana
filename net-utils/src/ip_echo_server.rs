use {
    crate::{HEADER_LENGTH, IP_ECHO_SERVER_RESPONSE_LENGTH},
    log::*,
    serde_derive::{Deserialize, Serialize},
    solana_sdk::deserialize_utils::default_on_eof,
    std::{
        io,
        net::{IpAddr, SocketAddr},
        time::Duration,
    },
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        runtime::{self, Runtime},
        time::timeout,
    },
};

pub type IpEchoServer = Runtime;

pub const MAX_PORT_COUNT_PER_MESSAGE: usize = 4;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct IpEchoServerMessage {
    tcp_ports: [u16; MAX_PORT_COUNT_PER_MESSAGE], // Fixed size list of ports to avoid vec serde
    udp_ports: [u16; MAX_PORT_COUNT_PER_MESSAGE], // Fixed size list of ports to avoid vec serde
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpEchoServerResponse {
    // Public IP address of request echoed back to the node.
    pub(crate) address: IpAddr,
    // Cluster shred-version of the node running the server.
    #[serde(deserialize_with = "default_on_eof")]
    pub(crate) shred_version: Option<u16>,
}

impl IpEchoServerMessage {
    pub fn new(tcp_ports: &[u16], udp_ports: &[u16]) -> Self {
        let mut msg = Self::default();
        assert!(tcp_ports.len() <= msg.tcp_ports.len());
        assert!(udp_ports.len() <= msg.udp_ports.len());

        msg.tcp_ports[..tcp_ports.len()].copy_from_slice(tcp_ports);
        msg.udp_ports[..udp_ports.len()].copy_from_slice(udp_ports);
        msg
    }
}

pub(crate) fn ip_echo_server_request_length() -> usize {
    const REQUEST_TERMINUS_LENGTH: usize = 1;
    HEADER_LENGTH
        + bincode::serialized_size(&IpEchoServerMessage::default()).unwrap() as usize
        + REQUEST_TERMINUS_LENGTH
}

async fn process_connection(
    mut socket: TcpStream,
    peer_addr: SocketAddr,
    shred_version: Option<u16>,
) -> io::Result<()> {
    info!("connection from {:?}", peer_addr);

    let mut data = vec![0u8; ip_echo_server_request_length()];

    let mut writer = {
        let (mut reader, writer) = socket.split();
        let _ = timeout(IO_TIMEOUT, reader.read_exact(&mut data)).await??;
        writer
    };

    let request_header: String = data[0..HEADER_LENGTH].iter().map(|b| *b as char).collect();
    if request_header != "\0\0\0\0" {
        // Explicitly check for HTTP GET/POST requests to more gracefully handle
        // the case where a user accidentally tried to use a gossip entrypoint in
        // place of a JSON RPC URL:
        if request_header == "GET " || request_header == "POST" {
            // Send HTTP error response
            timeout(
                IO_TIMEOUT,
                writer.write_all(b"HTTP/1.1 400 Bad Request\nContent-length: 0\n\n"),
            )
            .await??;
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Bad request header: {request_header}"),
        ));
    }

    let msg =
        bincode::deserialize::<IpEchoServerMessage>(&data[HEADER_LENGTH..]).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to deserialize IpEchoServerMessage: {err:?}"),
            )
        })?;

    trace!("request: {:?}", msg);

    // Fire a datagram at each non-zero UDP port
    match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(udp_socket) => {
            for udp_port in &msg.udp_ports {
                if *udp_port != 0 {
                    match udp_socket.send_to(&[0], SocketAddr::from((peer_addr.ip(), *udp_port))) {
                        Ok(_) => debug!("Successful send_to udp/{}", udp_port),
                        Err(err) => info!("Failed to send_to udp/{}: {}", udp_port, err),
                    }
                }
            }
        }
        Err(err) => {
            warn!("Failed to bind local udp socket: {}", err);
        }
    }

    // Try to connect to each non-zero TCP port
    for tcp_port in &msg.tcp_ports {
        if *tcp_port != 0 {
            debug!("Connecting to tcp/{}", tcp_port);

            let mut tcp_stream = timeout(
                IO_TIMEOUT,
                TcpStream::connect(&SocketAddr::new(peer_addr.ip(), *tcp_port)),
            )
            .await??;

            debug!("Connection established to tcp/{}", *tcp_port);
            tcp_stream.shutdown().await?;
        }
    }
    let response = IpEchoServerResponse {
        address: peer_addr.ip(),
        shred_version,
    };
    // "\0\0\0\0" header is added to ensure a valid response will never
    // conflict with the first four bytes of a valid HTTP response.
    let mut bytes = vec![0u8; IP_ECHO_SERVER_RESPONSE_LENGTH];
    bincode::serialize_into(&mut bytes[HEADER_LENGTH..], &response).unwrap();
    trace!("response: {:?}", bytes);
    writer.write_all(&bytes).await
}

async fn run_echo_server(tcp_listener: std::net::TcpListener, shred_version: Option<u16>) {
    info!("bound to {:?}", tcp_listener.local_addr().unwrap());
    let tcp_listener =
        TcpListener::from_std(tcp_listener).expect("Failed to convert std::TcpListener");

    loop {
        match tcp_listener.accept().await {
            Ok((socket, peer_addr)) => {
                runtime::Handle::current().spawn(async move {
                    if let Err(err) = process_connection(socket, peer_addr, shred_version).await {
                        info!("session failed: {:?}", err);
                    }
                });
            }
            Err(err) => warn!("listener accept failed: {:?}", err),
        }
    }
}

/// Starts a simple TCP server on the given port that echos the IP address of any peer that
/// connects.  Used by |get_public_ip_addr|
pub fn ip_echo_server(
    tcp_listener: std::net::TcpListener,
    // Cluster shred-version of the node running the server.
    shred_version: Option<u16>,
) -> IpEchoServer {
    tcp_listener.set_nonblocking(true).unwrap();

    let runtime = Runtime::new().expect("Failed to create Runtime");
    runtime.spawn(run_echo_server(tcp_listener, shred_version));
    runtime
}
