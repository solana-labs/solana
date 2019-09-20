use bytes::Bytes;
use log::*;
use serde_derive::{Deserialize, Serialize};
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio::reactor::Handle;
use tokio::runtime::Runtime;
use tokio_codec::{BytesCodec, Decoder};

pub type IpEchoServer = Runtime;

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct IpEchoServerMessage {
    tcp_ports: [u16; 4], // Fixed size list of ports to avoid vec serde
    udp_ports: [u16; 4], // Fixed size list of ports to avoid vec serde
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

/// Starts a simple TCP server on the given port that echos the IP address of any peer that
/// connects.  Used by |get_public_ip_addr|
pub fn ip_echo_server(tcp: std::net::TcpListener) -> IpEchoServer {
    info!("bound to {:?}", tcp.local_addr());
    let tcp =
        TcpListener::from_std(tcp, &Handle::default()).expect("Failed to convert std::TcpListener");

    let server = tcp
        .incoming()
        .map_err(|err| warn!("accept failed: {:?}", err))
        .for_each(move |socket| {
            let ip = socket.peer_addr().expect("Expect peer_addr()").ip();
            info!("connection from {:?}", ip);

            let framed = BytesCodec::new().framed(socket);
            let (writer, reader) = framed.split();

            let processor = reader
                .and_then(move |bytes| {
                    bincode::deserialize::<IpEchoServerMessage>(&bytes).or_else(|err| {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Failed to deserialize IpEchoServerMessage: {:?}", err),
                        ))
                    })
                })
                .and_then(move |msg| {
                    // Fire a datagram at each non-zero UDP port
                    if !msg.udp_ports.is_empty() {
                        match std::net::UdpSocket::bind("0.0.0.0:0") {
                            Ok(udp_socket) => {
                                for udp_port in &msg.udp_ports {
                                    if *udp_port != 0 {
                                        match udp_socket
                                            .send_to(&[0], SocketAddr::from((ip, *udp_port)))
                                        {
                                            Ok(_) => debug!("Successful send_to udp/{}", udp_port),
                                            Err(err) => {
                                                info!("Failed to send_to udp/{}: {}", udp_port, err)
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                warn!("Failed to bind local udp socket: {}", err);
                            }
                        }
                    }

                    // Try to connect to each non-zero TCP port
                    let tcp_futures: Vec<_> = msg
                        .tcp_ports
                        .iter()
                        .filter_map(|tcp_port| {
                            let tcp_port = *tcp_port;
                            if tcp_port == 0 {
                                None
                            } else {
                                Some(
                                    tokio::net::TcpStream::connect(&SocketAddr::new(ip, tcp_port))
                                        .and_then(move |tcp_stream| {
                                            debug!("Connection established to tcp/{}", tcp_port);
                                            let _ = tcp_stream.shutdown(std::net::Shutdown::Both);
                                            Ok(())
                                        })
                                        .timeout(Duration::from_secs(5))
                                        .or_else(move |err| {
                                            Err(io::Error::new(
                                                io::ErrorKind::Other,
                                                format!(
                                                    "Connection timeout to {}: {:?}",
                                                    tcp_port, err
                                                ),
                                            ))
                                        }),
                                )
                            }
                        })
                        .collect();
                    future::join_all(tcp_futures)
                })
                .and_then(move |_| {
                    let ip = bincode::serialize(&ip).unwrap_or_else(|err| {
                        warn!("Failed to serialize: {:?}", err);
                        vec![]
                    });
                    Ok(Bytes::from(ip))
                });

            let connection = writer
                .send_all(processor)
                .timeout(Duration::from_secs(5))
                .then(|result| {
                    if let Err(err) = result {
                        info!("Session failed: {:?}", err);
                    }
                    Ok(())
                });

            tokio::spawn(connection)
        });

    let mut rt = Runtime::new().expect("Failed to create Runtime");
    rt.spawn(server);
    rt
}
