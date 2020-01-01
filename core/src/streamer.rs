//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!

use crate::packet::{self, send_to, Packets, PacketsRecycler, PACKETS_PER_BATCH};
use crate::recvmmsg::NUM_RCVMMSGS;
use solana_measure::thread_mem_usage;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SendError, Sender};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};
use thiserror::Error;

pub type PacketReceiver = Receiver<Packets>;
pub type PacketSender = Sender<Packets>;

#[derive(Error, Debug)]
pub enum StreamerError {
    #[error("I/O error")]
    IO(#[from] std::io::Error),

    #[error("receive timeout error")]
    RecvTimeoutError(#[from] RecvTimeoutError),

    #[error("send packets error")]
    SendError(#[from] SendError<Packets>),
}

pub type Result<T> = std::result::Result<T, StreamerError>;

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
            // (for instance the leader transaction socket)
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Ok(len) = packet::recv_from(&mut msgs, sock) {
                if len == NUM_RCVMMSGS {
                    num_max_received += 1;
                }
                recv_count += len;
                call_count += 1;
                if len > 0 {
                    channel.send(msgs)?;
                }
                break;
            }
        }
        if recv_count > 1024 {
            datapoint_debug!(
                name,
                ("received", recv_count as i64, i64),
                ("call_count", i64::from(call_count), i64),
                ("elapsed", now.elapsed().as_millis() as i64, i64),
                ("max_received", i64::from(num_max_received), i64),
            );
            recv_count = 0;
            call_count = 0;
            num_max_received = 0;
        }
        now = Instant::now();
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
            thread_mem_usage::datapoint(name);
            let _ = recv_loop(&sock, exit, &packet_sender, &recycler.clone(), name);
        })
        .unwrap()
}

fn recv_send(sock: &UdpSocket, r: &PacketReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let msgs = r.recv_timeout(timer)?;
    send_to(&msgs, sock)?;
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

pub fn responder(name: &'static str, sock: Arc<UdpSocket>, r: PacketReceiver) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solana-responder-{}", name))
        .spawn(move || loop {
            thread_mem_usage::datapoint(name);
            if let Err(e) = recv_send(&sock, &r) {
                match e {
                    StreamerError::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                    StreamerError::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    _ => warn!("{} responder error: {:?}", name, e),
                }
            }
        })
        .unwrap()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::packet::{Packet, Packets, PACKET_DATA_SIZE};
    use crate::streamer::{receiver, responder};
    use solana_perf::recycler::Recycler;
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
            let mut msgs = Packets::default();
            for i in 0..5 {
                let mut b = Packet::default();
                {
                    b.data[0] = i as u8;
                    b.meta.size = PACKET_DATA_SIZE;
                    b.meta.set_addr(&addr);
                }
                msgs.packets.push(b);
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
