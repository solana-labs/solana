use {
    crossbeam_channel::{Receiver, Sender},
    log::debug,
    quinn::RecvStream,
    solana_sdk::{
        pubkey::Pubkey, signature::Signature, slot_history::Slot, transaction::VersionedTransaction,
    },
    solana_streamer::bidirectional_channel::{QuicReplyMessage, QUIC_REPLY_MESSAGE_SIZE},
    std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    },
};

pub const PACKET_DATA_SIZE: usize = 1280 - 40 - 8;

pub struct QuicHandlerMessage {
    pub transaction_signature: Signature,
    pub message: String,
    pub server_identity: Option<Pubkey>,
    pub server_socket: Option<SocketAddr>,
    pub quic_connection_error: bool,
    pub approximate_slot: Slot,
}

impl QuicHandlerMessage {
    pub fn new(message:QuicReplyMessage) -> Self {
        match message {
            QuicReplyMessage::TransactionExecutionMessage { leader_identity, transaction_signature, message, approximate_slot, .. } => {
                Self {
                    transaction_signature,
                    server_identity: Some(leader_identity),
                    message: Self::decode_vec_char_to_message(message),
                    approximate_slot,
                    quic_connection_error: false,
                    server_socket: None,
                }
            }
        }
    }

    fn decode_vec_char_to_message(message: Vec<u8>) -> String {
        let index_end = match message.iter().position(|x| *x == 0) {
            Some(x) => x,
            None => 128,
        };
        match String::from_utf8(message[0..index_end].to_vec()) {
            Ok(x) => x,
            Err(_) => "".to_string(),
        }
    }

    pub fn signature(&self) -> Signature {
        self.transaction_signature
    }

    pub fn message(&self) -> String {
        self.message.clone()
    }

    pub fn approximate_slot(&self) -> Slot {
        self.approximate_slot
    }
}

// This structure will handle the bidirectional messages that we get from the quic server
// It will save 1024 QuicReplyMessages sent by the server in the crossbeam receiver
// This class will also handle recv channel created by the QuicClient when connecting to the server in bidirectional mode
#[derive(Clone)]
pub struct BidirectionalChannelHandler {
    sender: Arc<Sender<QuicHandlerMessage>>,
    pub reciever: Receiver<QuicHandlerMessage>,
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
    pub fn mark_buffer_as_error(&self, buffer: &[u8], error: String, tpu_socket: SocketAddr) {
        let buffer = buffer.to_vec();
        let error = error.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let versioned_transaction = bincode::deserialize::<VersionedTransaction>(&buffer[..]);
            if let Ok(versioned_transaction) = versioned_transaction {
                let message = QuicHandlerMessage {
                    transaction_signature: versioned_transaction.signatures[0],
                    server_identity: None,
                    message: error,
                    server_socket: Some(tpu_socket),
                    quic_connection_error: true,
                    approximate_slot: 0,
                };
                let _ = sender.send(message);
            }
        });
    }

    pub fn start_serving(&self, recv_stream: RecvStream) {
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

            const LAST_BUFFER_SIZE: usize = QUIC_REPLY_MESSAGE_SIZE + 1;
            let mut last_buffer: [u8; LAST_BUFFER_SIZE] = [0; LAST_BUFFER_SIZE];
            let mut buffer_written = 0;
            let mut recv_stream = recv_stream;
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
                                    // move data into current buffer
                                    let mut buffer = vec![0; buffer_written + chunk.bytes.len()];
                                    if buffer_written > 0 {
                                        // copy remaining data from previous buffer
                                        buffer[0..buffer_written]
                                            .copy_from_slice(&last_buffer[0..buffer_written]);
                                    }
                                    buffer[buffer_written..buffer_written + chunk.bytes.len()]
                                        .copy_from_slice(&chunk.bytes);
                                    buffer_written = buffer_written + chunk.bytes.len();

                                    while buffer_written >= QUIC_REPLY_MESSAGE_SIZE {
                                        let message = bincode::deserialize::<QuicReplyMessage>(
                                            &buffer.as_slice(),
                                        );
                                        if let Ok(message) = message {
                                            let handler_message = QuicHandlerMessage::new(message);
                                            if let Err(_) = sender.send(handler_message) {
                                                // crossbeam channel closed
                                                break;
                                            }
                                        } else {
                                            // deserializing error
                                            debug!("deserializing error on BidirectionalChannelHandler");
                                        }
                                        buffer.copy_within(QUIC_REPLY_MESSAGE_SIZE.., 0);
                                        buffer_written -= QUIC_REPLY_MESSAGE_SIZE;
                                    }
                                    if buffer_written > 0 {
                                        // move remianing data into last buffer
                                        last_buffer[0..buffer_written]
                                            .copy_from_slice(&buffer[0..buffer_written]);
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
                } else {
                    break;
                }

                timeout = timeout.saturating_sub((Instant::now() - start).as_millis() as u64);
                start = Instant::now();
            }
            recv_channel_is_set.store(false, Ordering::Relaxed);
        });
    }
}
