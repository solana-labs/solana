use crossbeam_channel::Sender;
use futures_util::stream::StreamExt;
use quinn::{Endpoint, Incoming, ServerConfig};
use solana_perf::packet::PacketBatch;
use solana_sdk::packet::Packet;
use std::{
    error::Error,
    net::{SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};

/// Returns default server configuration along with its certificate.
#[allow(clippy::field_reassign_with_default)] // https://github.com/rust-lang/rust-clippy/issues/6527
fn configure_server() -> Result<(ServerConfig, Vec<u8>), Box<dyn Error>> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];

    let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)?;
    Arc::get_mut(&mut server_config.transport)
        .unwrap()
        .max_concurrent_uni_streams(0_u8.into());

    Ok((server_config, cert_der))
}

/// Constructs a QUIC endpoint configured to listen for incoming connections on a certain address
/// and port.
///
/// ## Returns
///
/// - a stream of incoming QUIC connections
/// - server certificate serialized into DER format
#[allow(unused)]
pub fn make_server_endpoint(bind_addr: SocketAddr) -> Result<(Incoming, Vec<u8>), Box<dyn Error>> {
    let (server_config, server_cert) = configure_server()?;
    let (_endpoint, incoming) = Endpoint::server(server_config, bind_addr)?;
    Ok((incoming, server_cert))
}

fn rt() -> Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}

pub fn spawn_server(
    sock: UdpSocket,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, Box<dyn Error>> {
    let (config, _cert) = configure_server()?;
    let handle = thread::spawn(move || {
        let runtime = rt();
        let (_, mut incoming) = {
            let _guard = runtime.enter();
            Endpoint::new(Default::default(), Some(config), sock).unwrap()
        };
        let handle = runtime.spawn(
            async move {
                let quinn::NewConnection {
                    mut uni_streams, ..
                } = incoming
                    .next()
                    .await
                    .expect("accept")
                    .await
                    .expect("connect");

                while let Some(Ok(mut stream)) = uni_streams.next().await {
                    let packet_sender = packet_sender.clone();
                    tokio::spawn(async move {
                        while let Some(chunk) = stream.read_chunk(usize::MAX, false).await.unwrap()
                        {
                            let mut batch = PacketBatch::with_capacity(1);
                            let mut packet = Packet::default();
                            packet.data.copy_from_slice(&chunk.bytes);
                            batch.packets.push(packet);
                            if let Err(e) = packet_sender.send(batch) {
                                info!("error: {}", e);
                            }
                        }
                    });
                }
            }, //.instrument(info!("server")),
        );
        //runtime.block_on(handle).unwrap();
        while !exit.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
        }
        handle.abort();
    });
    Ok(handle)
}
