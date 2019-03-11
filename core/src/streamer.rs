//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!

use crate::packet::{Blob, Meta, Packet, Packets, SharedBlobs, SharedPackets, PACKET_DATA_SIZE};
use crate::result::{Error, Result};
use bincode::{deserialize, serialized_size};
use solana_metrics::{influxdb, submit};
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};

pub type PacketReceiver = Receiver<SharedPackets>;
pub type PacketSender = Sender<SharedPackets>;
pub type BlobSender = Sender<SharedBlobs>;
pub type BlobReceiver = Receiver<SharedBlobs>;

fn recv_loop(
    sock: &UdpSocket,
    exit: Arc<AtomicBool>,
    channel: &PacketSender,
    channel_tag: &'static str,
) -> Result<()> {
    loop {
        let msgs = SharedPackets::default();
        loop {
            // Check for exit signal, even if socket is busy
            // (for instance the leader trasaction socket)
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if msgs.write().unwrap().recv_from(sock).is_ok() {
                let len = msgs.read().unwrap().packets.len();
                submit(
                    influxdb::Point::new(channel_tag)
                        .add_field("count", influxdb::Value::Integer(len as i64))
                        .to_owned(),
                );
                channel.send(msgs)?;
                break;
            }
        }
    }
}

pub fn receiver(
    sock: Arc<UdpSocket>,
    exit: &Arc<AtomicBool>,
    packet_sender: PacketSender,
    sender_tag: &'static str,
) -> JoinHandle<()> {
    let res = sock.set_read_timeout(Some(Duration::new(1, 0)));
    if res.is_err() {
        panic!("streamer::receiver set_read_timeout error");
    }
    let exit = exit.clone();
    Builder::new()
        .name("solana-receiver".to_string())
        .spawn(move || {
            let _ = recv_loop(&sock, exit, &packet_sender, sender_tag);
        })
        .unwrap()
}

fn recv_send(sock: &UdpSocket, r: &BlobReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    Blob::send_to(sock, msgs)?;
    Ok(())
}

pub fn recv_batch(recvr: &PacketReceiver) -> Result<(Vec<SharedPackets>, usize, u64)> {
    let timer = Duration::new(1, 0);
    let msgs = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got msgs");
    let mut len = msgs.read().unwrap().packets.len();
    let mut batch = vec![msgs];
    while let Ok(more) = recvr.try_recv() {
        trace!("got more msgs");
        len += more.read().unwrap().packets.len();
        batch.push(more);

        if len > 100_000 {
            break;
        }
    }
    trace!("batch len {}", batch.len());
    Ok((batch, len, duration_as_ms(&recv_start.elapsed())))
}

pub fn responder(name: &'static str, sock: Arc<UdpSocket>, r: BlobReceiver) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solana-responder-{}", name))
        .spawn(move || loop {
            if let Err(e) = recv_send(&sock, &r) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    _ => warn!("{} responder error: {:?}", name, e),
                }
            }
        })
        .unwrap()
}

//TODO, we would need to stick block authentication before we create the
//window.
fn recv_blobs(sock: &UdpSocket, s: &BlobSender) -> Result<()> {
    trace!("recv_blobs: receiving on {}", sock.local_addr().unwrap());
    let dq = Blob::recv_from(sock)?;
    if !dq.is_empty() {
        s.send(dq)?;
    }
    Ok(())
}

pub fn blob_receiver(
    sock: Arc<UdpSocket>,
    exit: &Arc<AtomicBool>,
    s: BlobSender,
) -> JoinHandle<()> {
    //DOCUMENTED SIDE-EFFECT
    //1 second timeout on socket read
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))
        .expect("set socket timeout");
    let exit = exit.clone();
    Builder::new()
        .name("solana-blob_receiver".to_string())
        .spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_blobs(&sock, &s);
        })
        .unwrap()
}

fn deserialize_single_packet_in_blob(data: &[u8], serialized_meta_size: usize) -> Result<Packet> {
    let meta = deserialize(&data[..serialized_meta_size])?;
    let mut packet_data = [0; PACKET_DATA_SIZE];
    packet_data
        .copy_from_slice(&data[serialized_meta_size..serialized_meta_size + PACKET_DATA_SIZE]);
    Ok(Packet::new(packet_data, meta))
}

fn deserialize_packets_in_blob(
    data: &[u8],
    serialized_packet_size: usize,
    serialized_meta_size: usize,
) -> Result<Vec<Packet>> {
    let mut packets: Vec<Packet> = Vec::with_capacity(data.len() / serialized_packet_size);
    let mut pos = 0;
    while pos + serialized_packet_size <= data.len() {
        let packet = deserialize_single_packet_in_blob(
            &data[pos..pos + serialized_packet_size],
            serialized_meta_size,
        )?;
        pos += serialized_packet_size;
        packets.push(packet);
    }
    Ok(packets)
}

fn recv_blob_packets(sock: &UdpSocket, s: &PacketSender) -> Result<()> {
    trace!(
        "recv_blob_packets: receiving on {}",
        sock.local_addr().unwrap()
    );

    let meta = Meta::default();
    let serialized_meta_size = serialized_size(&meta)? as usize;
    let serialized_packet_size = serialized_meta_size + PACKET_DATA_SIZE;
    let blobs = Blob::recv_from(sock)?;
    for blob in blobs {
        let r_blob = blob.read().unwrap();
        let data = {
            let msg_size = r_blob.size();
            &r_blob.data()[..msg_size]
        };

        let packets =
            deserialize_packets_in_blob(data, serialized_packet_size, serialized_meta_size);

        if packets.is_err() {
            continue;
        }

        let packets = packets?;
        s.send(Arc::new(RwLock::new(Packets::new(packets))))?;
    }

    Ok(())
}

pub fn blob_packet_receiver(
    sock: Arc<UdpSocket>,
    exit: &Arc<AtomicBool>,
    s: PacketSender,
) -> JoinHandle<()> {
    //DOCUMENTED SIDE-EFFECT
    //1 second timeout on socket read
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))
        .expect("set socket timeout");
    let exit = exit.clone();
    Builder::new()
        .name("solana-blob_packet_receiver".to_string())
        .spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_blob_packets(&sock, &s);
        })
        .unwrap()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::packet::{Blob, Meta, Packet, Packets, SharedBlob, PACKET_DATA_SIZE};
    use crate::streamer::{receiver, responder};
    use rand::Rng;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;

    fn get_msgs(r: PacketReceiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.read().unwrap().packets.len(),
                _ => info!("get_msgs error"),
            }
            if *num == 10 {
                break;
            }
        }
    }
    #[test]
    fn streamer_debug() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
        write!(io::sink(), "{:?}", Blob::default()).unwrap();
    }
    #[test]
    fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(Arc::new(read), &exit, s_reader, "streamer-test");
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder("streamer_send_test", Arc::new(send), r_responder);
            let mut msgs = Vec::new();
            for i in 0..10 {
                let b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.data[0] = i as u8;
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&addr);
                }
                msgs.push(b);
            }
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }

    #[test]
    fn test_streamer_deserialize_packets_in_blob() {
        let meta = Meta::default();
        let serialized_meta_size = serialized_size(&meta).unwrap() as usize;
        let serialized_packet_size = serialized_meta_size + PACKET_DATA_SIZE;
        let num_packets = 10;
        let mut rng = rand::thread_rng();
        let packets: Vec<_> = (0..num_packets)
            .map(|_| {
                let mut packet = Packet::default();
                for i in 0..packet.meta.addr.len() {
                    packet.meta.addr[i] = rng.gen_range(1, std::u16::MAX);
                }
                for i in 0..packet.data.len() {
                    packet.data[i] = rng.gen_range(1, std::u8::MAX);
                }
                packet
            })
            .collect();

        let mut blob = Blob::default();
        assert_eq!(blob.store_packets(&packets[..]), num_packets);
        let result = deserialize_packets_in_blob(
            &blob.data()[..blob.size()],
            serialized_packet_size,
            serialized_meta_size,
        )
        .unwrap();

        assert_eq!(result, packets);
    }
}
