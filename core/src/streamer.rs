//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!

use crate::packet::{Blob, Packets, PacketsRecycler, SharedBlobs, PACKETS_PER_BATCH};
use crate::recvmmsg::NUM_RCVMMSGS;
use crate::result::{Error, Result};
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};

pub type PacketReceiver = Receiver<Packets>;
pub type PacketSender = Sender<Packets>;
pub type BlobSender = Sender<SharedBlobs>;
pub type BlobReceiver = Receiver<SharedBlobs>;

fn recv_loop(
    sock: &UdpSocket,
    exit: Arc<AtomicBool>,
    channel: &PacketSender,
    recycler: &PacketsRecycler,
    name: &'static str,
) -> Result<()> {
    let mut recv_count = 0;
    let mut call_count = 0;
    let mut now = Instant::now();
    let mut num_max_received = 0; // Number of times maximum packets were received
    loop {
        let mut msgs = Packets::new_with_recycler(recycler.clone(), PACKETS_PER_BATCH, name);
        loop {
            // Check for exit signal, even if socket is busy
            // (for instance the leader trasaction socket)
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Ok(len) = msgs.recv_from(sock) {
                if len == NUM_RCVMMSGS {
                    num_max_received += 1;
                }
                recv_count += len;
                call_count += 1;
                channel.send(msgs)?;
                break;
            }
        }
        if recv_count > 1024 {
            datapoint_info!(
                "receiver-stats",
                ("received", recv_count as i64, i64),
                ("call_count", i64::from(call_count), i64),
                ("elapsed", now.elapsed().as_millis() as i64, i64),
                ("max_received", i64::from(num_max_received), i64),
            );
            recv_count = 0;
            call_count = 0;
            now = Instant::now();
            num_max_received = 0;
        }
    }
}

pub fn receiver(
    sock: Arc<UdpSocket>,
    exit: &Arc<AtomicBool>,
    packet_sender: PacketSender,
    recycler: PacketsRecycler,
    name: &'static str,
) -> JoinHandle<()> {
    let res = sock.set_read_timeout(Some(Duration::new(1, 0)));
    if res.is_err() {
        panic!("streamer::receiver set_read_timeout error");
    }
    let exit = exit.clone();
    Builder::new()
        .name("solana-receiver".to_string())
        .spawn(move || {
            let _ = recv_loop(&sock, exit, &packet_sender, &recycler.clone(), name);
        })
        .unwrap()
}

fn recv_send(sock: &UdpSocket, r: &BlobReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    Blob::send_to(sock, msgs)?;
    Ok(())
}

pub fn recv_batch(recvr: &PacketReceiver, max_batch: usize) -> Result<(Vec<Packets>, usize, u64)> {
    let timer = Duration::new(1, 0);
    let msgs = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got msgs");
    let mut len = msgs.packets.len();
    let mut batch = vec![msgs];
    while let Ok(more) = recvr.try_recv() {
        trace!("got more msgs");
        len += more.packets.len();
        batch.push(more);

        if len > max_batch {
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::packet::{Blob, Packet, Packets, SharedBlob, PACKET_DATA_SIZE};
    use crate::recycler::Recycler;
    use crate::streamer::{receiver, responder};
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;

    fn get_msgs(r: PacketReceiver, num: &mut usize) -> Result<()> {
        for _ in 0..10 {
            let m = r.recv_timeout(Duration::new(1, 0));
            if m.is_err() {
                continue;
            }

            *num -= m.unwrap().packets.len();

            if *num == 0 {
                break;
            }
        }

        Ok(())
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
        let t_receiver = receiver(Arc::new(read), &exit, s_reader, Recycler::default(), "test");
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder("streamer_send_test", Arc::new(send), r_responder);
            let mut msgs = Vec::new();
            for i in 0..5 {
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

        let mut num = 5;
        get_msgs(r_reader, &mut num).expect("get_msgs");
        assert_eq!(num, 0);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
