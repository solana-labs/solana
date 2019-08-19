use log::*;
use std::net::SocketAddr;
use tokio;
use tokio::net::TcpListener;
use tokio::prelude::{Future, Stream};
use tokio::runtime::Runtime;

pub type IpEchoServer = Runtime;

/// Starts a simple TCP server on the given port that echos the IP address of any peer that
/// connects.  Used by |get_public_ip_addr|
pub fn ip_echo_server(port: u16) -> IpEchoServer {
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
    let tcp =
        TcpListener::bind(&bind_addr).unwrap_or_else(|_| panic!("Unable to bind to {}", bind_addr));
    info!("bound to {:?}", bind_addr);

    let server = tcp
        .incoming()
        .map_err(|err| warn!("accept failed: {:?}", err))
        .for_each(move |socket| {
            let ip = socket
                .peer_addr()
                .and_then(|peer_addr| {
                    bincode::serialize(&peer_addr.ip()).map_err(|err| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to serialize: {:?}", err),
                        )
                    })
                })
                .unwrap_or_else(|_| vec![]);

            let write_future = tokio::io::write_all(socket, ip)
                .map_err(|err| warn!("write error: {:?}", err))
                .map(|_| ());

            tokio::spawn(write_future)
        });

    let mut rt = Runtime::new().expect("Failed to create Runtime");
    rt.spawn(server);
    rt
}
