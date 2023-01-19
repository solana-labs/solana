use {
    crossbeam_channel::{Receiver, Sender},
    log::debug,
    quinn::RecvStream,
    solana_sdk::signature::Signature,
    solana_streamer::bidirectional_channel::{
        QuicReplyMessage, QUIC_REPLY_MESSAGE_OFFSET, QUIC_REPLY_MESSAGE_SIGNATURE_OFFSET,
        QUIC_REPLY_MESSAGE_SIZE,
    },
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    },
};

pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;

// This structure will handle the bidirectional messages that we get from the quic server
// It will save 1024 QuicReplyMessages sent by the server in the crossbeam receiver
// This class will also handle recv channel created by the QuicClient when connecting to the server in bidirectional mode
#[derive(Clone)]
pub struct BidirectionalChannelHandler {
    sender: Arc<Sender<QuicReplyMessage>>,
    pub reciever: Receiver<QuicReplyMessage>,
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
            let mut timeout: u64 = 10_000;
            let mut start = Instant::now();

            let mut buffer: [u8; PACKET_DATA_SIZE] = [0; PACKET_DATA_SIZE];
            let mut buffer_written = 0;
            // when we reset the buffer we keep track of the last offset using this variable
            let mut last_offset = 0;

            loop {
                if let Ok(chunk) = tokio::time::timeout(
                    Duration::from_millis(timeout),
                    recv_stream.read_chunk(PACKET_DATA_SIZE, false),
                )
                .await
                {
                    match chunk {
                        Ok(maybe_chunk) => {
                            match maybe_chunk {
                                Some(chunk) => {
                                    let end_of_chunk = match (chunk.offset as usize)
                                        .checked_add(chunk.bytes.len())
                                    {
                                        Some(end) => end,
                                        None => break,
                                    };

                                    println!("BidirectionalChannelHandler got chunk length {} offset {} end of chunk {} last_offset {} ", chunk.bytes.len(), chunk.offset, end_of_chunk, last_offset);
                                    // move chunk into buffer
                                    buffer[(chunk.offset as usize - last_offset)
                                        ..(end_of_chunk - last_offset)]
                                        .copy_from_slice(&chunk.bytes);
                                    buffer_written =
                                        std::cmp::max(buffer_written, end_of_chunk - last_offset);

                                    while buffer_written >= QUIC_REPLY_MESSAGE_SIZE {
                                        let signature = bincode::deserialize::<Signature>(
                                            &buffer[QUIC_REPLY_MESSAGE_SIGNATURE_OFFSET
                                                ..QUIC_REPLY_MESSAGE_OFFSET],
                                        );
                                        let message: [u8; 128] = buffer
                                            [QUIC_REPLY_MESSAGE_OFFSET..QUIC_REPLY_MESSAGE_SIZE]
                                            .try_into()
                                            .unwrap();
                                        if let Ok(signature) = signature {
                                            println!("BidirectionalChannelHandler got a message for signature {}", signature);

                                            if let Err(_) =
                                                sender.send(QuicReplyMessage::new_with_bytes(
                                                    signature, message,
                                                ))
                                            {
                                                // crossbeam channel closed
                                                break;
                                            }
                                        } else {
                                            println!("deserializing error on BidirectionalChannelHandler");
                                            // deserializing error
                                            debug!("deserializing error on BidirectionalChannelHandler");
                                        }
                                        buffer.copy_within(QUIC_REPLY_MESSAGE_SIZE.., 0);
                                        buffer_written -= QUIC_REPLY_MESSAGE_SIZE;
                                        last_offset = end_of_chunk;
                                    }
                                }
                                None => {
                                    // done receiving chunks
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            debug!("BidirectionalChannelHandler recieved error {}", e);
                            break;
                        }
                    }
                    // match buf_size {
                    //     Ok(buf_size) => {
                    //         if let Some(buf_size) = buf_size {
                    //             let buffer = &buf[0..buf_size];
                    //             match bincode::deserialize::<QuicReplyMessageBatch>(buffer) {
                    //                 Ok(messages) => {
                    //                     println!(
                    //                         "recieved a buffered data of size {}",
                    //                         messages.messages.len()
                    //                     );
                    //                     let _ = sender.send(messages);
                    //                 }
                    //                 _ => {
                    //                     println!("unformatted message");
                    //                     // unformatted message
                    //                     break;
                    //                 }
                    //             }
                    //         }
                    //     }
                    //     Err(e) => {
                    //         println!("BidirectionalChannelHandler stream got error {}", e);
                    //         break;
                    //     }
                    // }
                } else {
                    println!("BidirectionalChannelHandler timeout");
                    break;
                }

                timeout = timeout.saturating_sub((Instant::now() - start).as_millis() as u64);
                start = Instant::now();
            }
            println!("BidirectionalChannelHandler finished");
            recv_channel_is_set.store(false, Ordering::Relaxed);
            true
        });
    }
}
