use crossbeam_channel::Sender;
use futures_util::stream::StreamExt;
use quinn::{Endpoint, Incoming, ServerConfig};
use pkcs8::{
    der::Document,
    AlgorithmIdentifier,
    ObjectIdentifier
};
use rcgen::{
    CertificateParams,
    DistinguishedName,
    DnType,
    SanType,
};
use pem::Pem;
use solana_perf::packet::PacketBatch;
use solana_sdk::packet::Packet;
use solana_sdk::signature::Keypair;
use std::{
    error::Error,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};

/// Returns default server configuration along with its PEM certificate chain.
#[allow(clippy::field_reassign_with_default)] // https://github.com/rust-lang/rust-clippy/issues/6527
fn configure_server(identity_keypair: &Keypair, gossip_host: IpAddr) -> Result<(ServerConfig, String), Box<dyn Error>> {
    let (cert_chain, priv_key) = new_cert(identity_keypair, gossip_host)?;
    let cert_chain_pem_parts: Vec<Pem> = cert_chain.clone()
        .into_iter()
        .map(|cert| Pem {
            tag: "CERTIFICATE".to_string(),
            contents: cert.0
        })
        .collect();
    let cert_chain_pem = pem::encode_many(&cert_chain_pem_parts);

    let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)?;
    Arc::get_mut(&mut server_config.transport)
        .unwrap()
        .max_concurrent_uni_streams(0_u8.into());

    Ok((server_config, cert_chain_pem))
}

fn new_cert(identity_keypair: &Keypair, san: IpAddr) -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey), Box<dyn Error>> {
    let cert_params = new_cert_params(identity_keypair, san);
    let cert = rcgen::Certificate::from_params(cert_params)?;
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der)];
    Ok((cert_chain, priv_key))
}

fn new_cert_params(identity_keypair: &Keypair, san: IpAddr) -> CertificateParams {
    // TODO(terorie): Is it safe to sign the TLS cert with the identity private key?

    // Unfortunately, rcgen does not accept a "raw" Ed25519 key.
    // We have to convert it to DER and pass it to the library.

    // Convert private key into PKCS#8 v1 object.
    // RFC 8410, Section 7: Private Key Format
    // https://datatracker.ietf.org/doc/html/rfc8410#section-7
    let mut private_key = Vec::<u8>::with_capacity(34);
    private_key.extend_from_slice(&[0x04, 0x20]); // ASN.1 OCTET STRING
    private_key.extend_from_slice(identity_keypair.secret().as_bytes());
    let key_pkcs8 = pkcs8::PrivateKeyInfo {
        algorithm: AlgorithmIdentifier {
            oid: ObjectIdentifier::from_arcs(&[1, 3, 101, 112]).unwrap(),
            parameters: None,
        },
        private_key: &private_key,
        public_key: None,
    };
    let key_pkcs8_der = key_pkcs8.to_der()
        .expect("Failed to convert keypair to DER")
        .to_der();

    // Parse private key into rcgen::KeyPair struct.
    let keypair = rcgen::KeyPair::from_der(&key_pkcs8_der)
        .expect("Failed to parse keypair from DER");

    let mut cert_params: CertificateParams = Default::default();
    cert_params.subject_alt_names = vec![SanType::IpAddress(san)];
    cert_params.alg = &rcgen::PKCS_ED25519;
    cert_params.key_pair = Some(keypair);
    cert_params.distinguished_name = DistinguishedName::new();
    cert_params.distinguished_name.push(DnType::CommonName, "Solana node");
    cert_params
}

/// Constructs a QUIC endpoint configured to listen for incoming connections on a certain address
/// and port.
///
/// ## Returns
///
/// - a stream of incoming QUIC connections
/// - server certificate serialized into DER format
#[allow(unused)]
pub fn make_server_endpoint(bind_addr: SocketAddr, keypair: &Keypair) -> Result<(Incoming, String), Box<dyn Error>> {
    // TODO(terorie) Bind address is not appropriate for SAN (could be 0.0.0.0)
    let (server_config, server_cert) = configure_server(keypair, bind_addr.ip())?;
    let (_endpoint, incoming) = Endpoint::server(server_config, bind_addr)?;
    Ok((incoming, server_cert))
}

fn rt() -> Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}

pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, Box<dyn Error>> {
    let (config, _cert) = configure_server(keypair, gossip_host)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_configure_server() {
        let keypair = Keypair::new();
        let san = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let (_server_config, cert) = configure_server(&keypair, san).unwrap();
        println!("{}", cert);
    }
}
