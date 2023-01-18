use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use quinn::RecvStream;
use solana_streamer::bidirectional_channel::QuicReplyMessageBatch;

// This structure will handle the bidirectional messages that we get from the quic server
// It will save 1024 QuicReplyMessages sent by the server in the crossbeam receiver
// This class will also handle recv channel created by the QuicClient when connecting to the server in bidirectional mode
#[derive(Clone)]
pub struct BidirectionalChannelHandler {
    sender: Arc<Sender<QuicReplyMessageBatch>>,
    pub reciever: Receiver<QuicReplyMessageBatch>,
    recv_channel_is_set: Arc<AtomicBool>,
}

impl BidirectionalChannelHandler {
    pub fn new() -> Self {
        let (sender, reciever) = crossbeam_channel::unbounded();
        Self {
            sender: Arc::new(sender),
            reciever,
            recv_channel_is_set: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_serving(&self) -> bool {
        self.recv_channel_is_set.load(Ordering::Relaxed)
    }

    pub fn start_serving(&self, mut recv_stream: RecvStream) {
        if self.is_serving() {
            return;
        }

        let recv_channel_is_set = self.recv_channel_is_set.clone();
        let sender = self.sender.clone();

        recv_channel_is_set.store(true, Ordering::Relaxed);
        // create task to fetch errors from the leader
        tokio::spawn(async move {
            // wait for 10 s max
            let mut timeout: u64 = 10;
            let mut start = Instant::now();
            let mut buf: [u8; 256] = [0; 256];
            let buf: &mut [u8] = &mut buf;
            loop {
                if timeout == 0 {
                    break;
                } else if let Ok(buf_size) =
                    tokio::time::timeout(Duration::from_secs(timeout), recv_stream.read(buf)).await
                {
                    match buf_size {
                        Ok(buf_size) => {
                            if let Some(buf_size) = buf_size {
                                let buffer = &buf[0..buf_size];
                                match bincode::deserialize::<QuicReplyMessageBatch>(buffer) {
                                    Ok(messages) => {
                                        let _ = sender.send(messages);
                                    }
                                    _ => {
                                        // unformatted message
                                        break;
                                    }
                                }
                            }
                        }
                        Err(_e) => {
                            break;
                        }
                    }
                    timeout = timeout.saturating_sub((Instant::now() - start).as_millis() as u64);
                    start = Instant::now();
                } else {
                    break;
                }
            }
            recv_channel_is_set.store(false, Ordering::Relaxed);
        });
    }
}
