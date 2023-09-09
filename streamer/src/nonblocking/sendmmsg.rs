//! The `sendmmsg` module provides a nonblocking sendmmsg() API implementation

use {
    crate::sendmmsg::SendPktsError,
    futures_util::future::join_all,
    std::{borrow::Borrow, iter::repeat, net::SocketAddr},
    tokio::net::UdpSocket,
};

pub async fn batch_send<S, T>(sock: &UdpSocket, packets: &[(T, S)]) -> Result<(), SendPktsError>
where
    S: Borrow<SocketAddr>,
    T: AsRef<[u8]>,
{
    let mut num_failed = 0;
    let mut erropt = None;
    let futures = packets
        .iter()
        .map(|(p, a)| sock.send_to(p.as_ref(), a.borrow()))
        .collect::<Vec<_>>();
    let results = join_all(futures).await;
    for result in results {
        if let Err(e) = result {
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

pub async fn multi_target_send<S, T>(
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
    batch_send(sock, &pkts).await
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            nonblocking::{
                recvmmsg::{recv_mmsg, recv_mmsg_exact},
                sendmmsg::{batch_send, multi_target_send},
            },
            packet::Packet,
            sendmmsg::SendPktsError,
        },
        assert_matches::assert_matches,
        solana_sdk::packet::PACKET_DATA_SIZE,
        std::{
            io::ErrorKind,
            net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
        },
        tokio::net::UdpSocket,
    };

    #[tokio::test]
    async fn test_send_mmsg_one_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = reader.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.expect("bind");

        let packets: Vec<_> = (0..32).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let packet_refs: Vec<_> = packets.iter().map(|p| (&p[..], &addr)).collect();

        let sent = batch_send(&sender, &packet_refs[..]).await.ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg_exact(&reader, &mut packets[..]).await.unwrap();
        assert_eq!(32, recv);
    }

    #[tokio::test]
    async fn test_send_mmsg_multi_dest() {
        let reader = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").await.expect("bind");

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

        let sent = batch_send(&sender, &packet_refs[..]).await.ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 16];
        let recv = recv_mmsg_exact(&reader, &mut packets[..]).await.unwrap();
        assert_eq!(16, recv);

        let mut packets = vec![Packet::default(); 16];
        let recv = recv_mmsg_exact(&reader2, &mut packets[..]).await.unwrap();
        assert_eq!(16, recv);
    }

    #[tokio::test]
    async fn test_multicast_msg() {
        let reader = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr = reader.local_addr().unwrap();

        let reader2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr2 = reader2.local_addr().unwrap();

        let reader3 = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr3 = reader3.local_addr().unwrap();

        let reader4 = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
        let addr4 = reader4.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").await.expect("bind");

        let packet = Packet::default();

        let sent = multi_target_send(
            &sender,
            packet.data(..).unwrap(),
            &[&addr, &addr2, &addr3, &addr4],
        )
        .await
        .ok();
        assert_eq!(sent, Some(()));

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader2, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader3, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);

        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(&reader4, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);
    }

    #[tokio::test]
    async fn test_intermediate_failures_mismatched_bind() {
        let packets: Vec<_> = (0..3).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let ip4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let ip6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080);
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ip4),
            (&packets[1][..], &ip6),
            (&packets[2][..], &ip4),
        ];
        let dest_refs: Vec<_> = vec![&ip4, &ip6, &ip4];

        let sender = UdpSocket::bind("0.0.0.0:0").await.expect("bind");
        if let Err(SendPktsError::IoError(_, num_failed)) =
            batch_send(&sender, &packet_refs[..]).await
        {
            assert_eq!(num_failed, 1);
        }
        if let Err(SendPktsError::IoError(_, num_failed)) =
            multi_target_send(&sender, &packets[0], &dest_refs).await
        {
            assert_eq!(num_failed, 1);
        }
    }

    #[tokio::test]
    async fn test_intermediate_failures_unreachable_address() {
        let packets: Vec<_> = (0..5).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        let ipv4local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let ipv4broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), 8080);
        let sender = UdpSocket::bind("0.0.0.0:0").await.expect("bind");

        // test intermediate failures for batch_send
        let packet_refs: Vec<_> = vec![
            (&packets[0][..], &ipv4local),
            (&packets[1][..], &ipv4broadcast),
            (&packets[2][..], &ipv4local),
            (&packets[3][..], &ipv4broadcast),
            (&packets[4][..], &ipv4local),
        ];
        if let Err(SendPktsError::IoError(ioerror, num_failed)) =
            batch_send(&sender, &packet_refs[..]).await
        {
            assert_matches!(ioerror.kind(), ErrorKind::PermissionDenied);
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
            batch_send(&sender, &packet_refs[..]).await
        {
            assert_matches!(ioerror.kind(), ErrorKind::PermissionDenied);
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
            batch_send(&sender, &packet_refs[..]).await
        {
            assert_matches!(ioerror.kind(), ErrorKind::PermissionDenied);
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
            multi_target_send(&sender, &packets[0], &dest_refs).await
        {
            assert_matches!(ioerror.kind(), ErrorKind::PermissionDenied);
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
            multi_target_send(&sender, &packets[0], &dest_refs).await
        {
            assert_matches!(ioerror.kind(), ErrorKind::PermissionDenied);
            assert_eq!(num_failed, 3);
        }
    }
}
