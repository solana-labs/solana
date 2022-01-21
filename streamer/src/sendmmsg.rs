//! The `sendmmsg` module provides sendmmsg() API implementation

#[cfg(target_os = "linux")]
use {
    itertools::izip,
    libc::{iovec, mmsghdr, sockaddr_in, sockaddr_in6, sockaddr_storage},
    nix::sys::socket::InetAddr,
    std::os::unix::io::AsRawFd,
};
use {
    std::{
        borrow::Borrow,
        io,
        iter::repeat,
        net::{SocketAddr, UdpSocket},
        sync::Arc,
    },
    thiserror::Error,
    quinn::{ClientConfig, Endpoint, NewConnection, EndpointConfig},
    anyhow::{Context, Result, anyhow},
    bytes::Bytes,
    solana_sdk::packet::MAX_TRANSACTION_SIZE,
    solana_sdk::packet::PacketInterface,
    rustls::*,
};

#[derive(Debug, Error)]
pub enum SendPktsError {
    /// IO Error during send: first error, num failed packets
    #[error("IO Error, some packets could not be sent")]
    IoError(io::Error, usize),
}

#[cfg(not(target_os = "linux"))]
pub fn batch_send<S, T>(sock: &UdpSocket, packets: &[(T, S)]) -> Result<(), SendPktsError>
where
    S: Borrow<SocketAddr>,
    T: AsRef<[u8]>,
{
    let mut num_failed = 0;
    let mut erropt = None;
    for (p, a) in packets {
        if let Err(e) = sock.send_to(p.as_ref(), a.borrow()) {
            num_failed += 1;
            if erropt.is_none() {
                erropt = Some(e);
            }
        }
    }

    if let Some(err) = erropt {
        Err(SendPktsError::IoError(err, num_failed))
    } else {
        Ok(())
    }
}


/// Dummy certificate verifier that treats any certificate as valid.
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    ClientConfig::new(Arc::new(crypto))
}

#[tokio::main]
pub async fn batch_send_quic<P: PacketInterface + 'static>(sock: &UdpSocket, packets: Vec<&P>, dest: &SocketAddr) -> Result<()>
{
    /*let url = options.url;
    let remote = (url.host_str().unwrap(), url.port().unwrap_or(4433))
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("couldn't resolve to an address"))?;*/
/*
    let mut roots = rustls::RootCertStore::empty();
    if let Some(ca_path) = options.ca {
        roots.add(&rustls::Certificate(fs::read(&ca_path)?))?;
    } else {
        let dirs = directories_next::ProjectDirs::from("org", "quinn", "quinn-examples").unwrap();
        match fs::read(dirs.data_local_dir().join("cert.der")) {
            Ok(cert) => {
                roots.add(&rustls::Certificate(cert))?;
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                info!("local server certificate not found");
            }
            Err(e) => {
                error!("failed to open local server certificate: {}", e);
            }
        }
    }*/
    let client_crypto = configure_client();

    /*
    client_crypto.alpn_protocols = common::ALPN_QUIC_HTTP.iter().map(|&x| x.into()).collect();
    if options.keylog {
        client_crypto.key_log = Arc::new(rustls::KeyLogFile::new());
    }*/

    let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, sock.try_clone()?)?.0;
    endpoint.set_default_client_config(client_crypto);

    //let request = format!("GET {}\r\n", url.path());
    //let start = Instant::now();
    //let rebind = options.rebind;
    /*
    let host = options
        .host
        .as_ref()
        .map_or_else(|| url.host_str(), |x| Some(x))
        .ok_or_else(|| anyhow!("no hostname specified"))?;

    eprintln!("connecting to {} at {}", host, remote);
    */
    let new_conn = endpoint
        .connect(*dest, "host")?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    //eprintln!("connected at {:?}", start.elapsed());
    let quinn::NewConnection {
        connection: conn, ..
    } = new_conn;
    /*let (mut send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open stream: {}", e))?;*/

        let max_dgram_size = conn.max_datagram_size();
        assert!(max_dgram_size.is_some() && max_dgram_size.unwrap() >= MAX_TRANSACTION_SIZE);

        /*
    if rebind {
        let socket = std::net::UdpSocket::bind("[::]:0").unwrap();
        let addr = socket.local_addr().unwrap();
        eprintln!("rebinding to {}", addr);
        endpoint.rebind(socket).expect("rebind failed");
    }
*/
/*
    send.write_all(request.as_bytes())
        .await
        .map_err(|e| anyhow!("failed to send request: {}", e))?;
    send.finish()
        .await
        .map_err(|e| anyhow!("failed to shutdown stream: {}", e))?;
    let response_start = Instant::now();
    eprintln!("request sent at {:?}", response_start - start);
    let resp = recv
        .read_to_end(usize::max_value())
        .await
        .map_err(|e| anyhow!("failed to read response: {}", e))?;
    let duration = response_start.elapsed();
    eprintln!(
        "response received in {:?} - {} KiB/s",
        duration,
        resp.len() as f32 / (duration_secs(&duration) * 1024.0)
    );
    io::stdout().write_all(&resp).unwrap();
    io::stdout().flush().unwrap();
*/
    //todo: is there a way to get rid of this copy?
    packets.iter().try_for_each(|packet| {conn.send_datagram(Bytes::copy_from_slice(&packet.get_data()[0..packet.get_meta().size]))})?;

    conn.close(0u32.into(), b"done");

    // Give the server a fair chance to receive the close packet
    endpoint.wait_idle().await;

    Ok(())
}



#[cfg(target_os = "linux")]
fn mmsghdr_for_packet(
    packet: &[u8],
    dest: &SocketAddr,
    iov: &mut iovec,
    addr: &mut sockaddr_storage,
    hdr: &mut mmsghdr,
) {
    const SIZE_OF_SOCKADDR_IN: usize = std::mem::size_of::<sockaddr_in>();
    const SIZE_OF_SOCKADDR_IN6: usize = std::mem::size_of::<sockaddr_in6>();

    *iov = iovec {
        iov_base: packet.as_ptr() as *mut libc::c_void,
        iov_len: packet.len(),
    };
    hdr.msg_hdr.msg_iov = iov;
    hdr.msg_hdr.msg_iovlen = 1;
    hdr.msg_hdr.msg_name = addr as *mut _ as *mut _;

    match InetAddr::from_std(dest) {
        InetAddr::V4(dest) => {
            unsafe {
                std::ptr::write(addr as *mut _ as *mut _, dest);
            }
            hdr.msg_hdr.msg_namelen = SIZE_OF_SOCKADDR_IN as u32;
        }
        InetAddr::V6(dest) => {
            unsafe {
                std::ptr::write(addr as *mut _ as *mut _, dest);
            }
            hdr.msg_hdr.msg_namelen = SIZE_OF_SOCKADDR_IN6 as u32;
        }
    };
}

#[cfg(target_os = "linux")]
fn sendmmsg_retry(sock: &UdpSocket, hdrs: &mut Vec<mmsghdr>) -> Result<(), SendPktsError> {
    let sock_fd = sock.as_raw_fd();
    let mut total_sent = 0;
    let mut erropt = None;

    let mut pkts = &mut hdrs[..];
    while !pkts.is_empty() {
        let npkts = match unsafe { libc::sendmmsg(sock_fd, &mut pkts[0], pkts.len() as u32, 0) } {
            -1 => {
                if erropt.is_none() {
                    erropt = Some(io::Error::last_os_error());
                }
                // skip over the failing packet
                1_usize
            }
            n => {
                // if we fail to send all packets we advance to the failing
                // packet and retry in order to capture the error code
                total_sent += n as usize;
                n as usize
            }
        };
        pkts = &mut pkts[npkts..];
    }

    if let Some(err) = erropt {
        Err(SendPktsError::IoError(err, hdrs.len() - total_sent))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub fn batch_send<S, T>(sock: &UdpSocket, packets: &[(T, S)]) -> Result<(), SendPktsError>
where
    S: Borrow<SocketAddr>,
    T: AsRef<[u8]>,
{
    let size = packets.len();
    #[allow(clippy::uninit_assumed_init)]
    let mut iovs = vec![unsafe { std::mem::MaybeUninit::uninit().assume_init() }; size];
    let mut addrs = vec![unsafe { std::mem::zeroed() }; size];
    let mut hdrs = vec![unsafe { std::mem::zeroed() }; size];
    for ((pkt, dest), hdr, iov, addr) in izip!(packets, &mut hdrs, &mut iovs, &mut addrs) {
        mmsghdr_for_packet(pkt.as_ref(), dest.borrow(), iov, addr, hdr);
    }
    sendmmsg_retry(sock, &mut hdrs)
}

pub fn multi_target_send<S, T>(
    sock: &UdpSocket,
    packet: T,
    dests: &[S],
) -> Result<(), SendPktsError>
where
    S: Borrow<SocketAddr>,
    T: AsRef<[u8]>,
{
    let dests = dests.iter().map(Borrow::borrow);
    let pkts: Vec<_> = repeat(&packet).zip(dests).collect();
    batch_send(sock, &pkts)
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            packet::Packet,
            recvmmsg::recv_mmsg,
            sendmmsg::{batch_send, multi_target_send, SendPktsError},
        },
        solana_sdk::packet::PACKET_DATA_SIZE,
        std::{
            io::ErrorKind,
            net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
        },
    };

    #[test]
    pub fn test_send_mmsg_one_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let packets: Vec<_> = (0..32).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let packet_refs: Vec<_> = packets.iter().map(|p| (&p[..], &addr)).collect();

        let sent = batch_send(&sender, &packet_refs[..]).ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(32, recv);
    }

    #[test]
    pub fn test_send_mmsg_multi_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let packets: Vec<_> = (0..32).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let packet_refs: Vec<_> = packets
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i < 16 {
                    (&p[..], &addr)
                } else {
                    (&p[..], &addr2)
                }
            })
            .collect();

        let sent = batch_send(&sender, &packet_refs[..]).ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(16, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader2, &mut packets[..]).unwrap().1;
        assert_eq!(16, recv);
    }

    #[test]
    pub fn test_multicast_msg() {
        let reader = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let reader3 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr3 = reader3.local_addr().unwrap();

        let reader4 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr4 = reader4.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");

        let packet = Packet::default();

        let sent = multi_target_send(
            &sender,
            &packet.data[..packet.meta.size],
            &[&addr, &addr2, &addr3, &addr4],
        )
        .ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader2, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader3, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader4, &mut packets[..]).unwrap().1;
        assert_eq!(1, recv);
    }

    #[test]
    fn test_intermediate_failures_mismatched_bind() {
        let packets: Vec<_> = (0..3).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let ip4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let ip6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080);
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ip4),
            (&packets[1][..], &ip6),
            (&packets[2][..], &ip4),
        ];
        let dest_refs: Vec<_> = vec![&ip4, &ip6, &ip4];

        let sender = UdpSocket::bind("0.0.0.0:0").expect("bind");
        if let Err(SendPktsError::IoError(_, num_failed)) = batch_send(&sender, &packet_refs[..]) {
            assert_eq!(num_failed, 1);
        }
        if let Err(SendPktsError::IoError(_, num_failed)) =
            multi_target_send(&sender, &packets[0], &dest_refs)
        {
            assert_eq!(num_failed, 1);
        }
    }

    #[test]
    fn test_intermediate_failures_unreachable_address() {
        let packets: Vec<_> = (0..5).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let ipv4local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let ipv4broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), 8080);
        let sender = UdpSocket::bind("0.0.0.0:0").expect("bind");

        // test intermediate failures for batch_send
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ipv4local),
            (&packets[1][..], &ipv4broadcast),
            (&packets[2][..], &ipv4local),
            (&packets[3][..], &ipv4broadcast),
            (&packets[4][..], &ipv4local),
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            batch_send(&sender, &packet_refs[..])
        {
            assert!(matches!(ioerror.kind(), ErrorKind::PermissionDenied));
            assert_eq!(num_failed, 2);
        }

        // test leading and trailing failures for batch_send
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ipv4broadcast),
            (&packets[1][..], &ipv4local),
            (&packets[2][..], &ipv4broadcast),
            (&packets[3][..], &ipv4local),
            (&packets[4][..], &ipv4broadcast),
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            batch_send(&sender, &packet_refs[..])
        {
            assert!(matches!(ioerror.kind(), ErrorKind::PermissionDenied));
            assert_eq!(num_failed, 3);
        }

        // test consecutive intermediate failures for batch_send
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ipv4local),
            (&packets[1][..], &ipv4local),
            (&packets[2][..], &ipv4broadcast),
            (&packets[3][..], &ipv4broadcast),
            (&packets[4][..], &ipv4local),
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            batch_send(&sender, &packet_refs[..])
        {
            assert!(matches!(ioerror.kind(), ErrorKind::PermissionDenied));
            assert_eq!(num_failed, 2);
        }

        // test intermediate failures for multi_target_send
        let dest_refs: Vec<_> = vec![
            &ipv4local,
            &ipv4broadcast,
            &ipv4local,
            &ipv4broadcast,
            &ipv4local,
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            multi_target_send(&sender, &packets[0], &dest_refs)
        {
            assert!(matches!(ioerror.kind(), ErrorKind::PermissionDenied));
            assert_eq!(num_failed, 2);
        }

        // test leading and trailing failures for multi_target_send
        let dest_refs: Vec<_> = vec![
            &ipv4broadcast,
            &ipv4local,
            &ipv4broadcast,
            &ipv4local,
            &ipv4broadcast,
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            multi_target_send(&sender, &packets[0], &dest_refs)
        {
            assert!(matches!(ioerror.kind(), ErrorKind::PermissionDenied));
            assert_eq!(num_failed, 3);
        }
    }
}
