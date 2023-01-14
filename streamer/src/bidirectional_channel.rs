// this is a service to handle bidirections quinn channel
use crossbeam_channel::{Receiver};
use quinn::SendStream;
use solana_perf::packet::{PacketBatch, Packet};
use solana_sdk::{signature::Signature, message::Message};
use std::{sync::{Mutex, Arc, RwLock}, collections::HashMap, net::SocketAddr};
use solana_sdk::hash::Hash;

pub struct QuicReplyMessage {
    transaction_sig : Option<Signature>,
    message_hash: Option<Hash>,
    message: String,   
}

pub struct QuicBidirectionalData {
    pub message_hash_map : HashMap<Hash, u64>,
    pub transaction_signature_map : HashMap<Signature, u64>,
    pub sender_ids_map : HashMap<u64, Arc<Mutex<SendStream>>>,
    pub sender_socket_address_map: HashMap<SocketAddr, u64>,
    pub last_id : u64,
}

pub struct QuicBidirectionalReplyService {
    data : Arc<RwLock<QuicBidirectionalData>>,
    pub service_reciever : Receiver<QuicReplyMessage>,
}

pub fn get_signature_from_packet(packet: &Packet) -> Option<Signature> {
    // add instruction signature for message
    match packet.data(1..33) {
        Some(signature_bytes) => {
            let sig =  Signature::new(&signature_bytes);
            Some(sig)
        },
        None => {
            None
        }
    }
}

impl QuicBidirectionalReplyService {

    pub fn new(
        service_reciever : Receiver<QuicReplyMessage>,
    ) -> Self {
        Self{
            service_reciever: service_reciever,
            data: Arc::new(RwLock::new(QuicBidirectionalData {
                message_hash_map : HashMap::new(),
                transaction_signature_map : HashMap::new(),
                sender_ids_map : HashMap::new(),
                sender_socket_address_map : HashMap::new(),
                last_id : 1,
            })),
        }
    }

    pub fn add_stream( &self, quic_address: &SocketAddr, send_stream: SendStream ) {
        let data = self.data.write();
        if let Ok(data) = data {
            let send_stream = Arc::new(Mutex::new(send_stream));
            match data.sender_socket_address_map.get( quic_address ) {
                Some(x) => {
                    // replace current stream
                    data.sender_ids_map.insert( *x, send_stream);
                },
                None => {
                    let current_id = data.last_id;
                    data.last_id += 1;
                    data.sender_ids_map.insert( current_id, send_stream);
                    data.sender_socket_address_map.insert( quic_address.clone(), current_id);
                }
            }

        }
    }

    pub fn add_packets( &self, quic_address: &SocketAddr, packets: &PacketBatch ) {
        // check if socket is registered;
        let id = {
            let data = self.data.read();
            if let Ok(data) = data {
                data.sender_socket_address_map.get(quic_address).map_or(0, |x| *x)
            }
            else {
                0
            }
        };
        if id == 0 {
            return;
        }
        let data = self.data.write();
        if let Ok(data) = data {
            packets.iter().for_each(|packet| {
                let signature = get_signature_from_packet(packet);
                signature.map(|x|{
                    data.transaction_signature_map.insert(x, id)
                });

                if let Some(data_packet) =  packet.data(..) {
                    let hash = Message::hash_raw_message(data_packet);
                    data.message_hash_map.insert(hash, id);
                }
            });
        }
    }

    fn serve(self) {
        let service_reciever = self.service_reciever;
        tokio::spawn(async move {
            loop {
                let service_reciever = service_reciever;
                let bidirectional_message = service_reciever.recv();
                match bidirectional_message {
                    Ok(bidirectional_message) => {
                        let data = self.data.read();
                        
                        if let Ok(data) = data {
                            let send_stream = {
                                // if the message has transaction signature then find stream from transaction signature
                                // else find stream by packet hash
                                let send_stream_id = if let Some(sig) = bidirectional_message.transaction_sig {
                                    data.transaction_signature_map.get(&sig).map(|x| *x)
                                } else if let Some(hash) = bidirectional_message.message_hash {
                                    data.message_hash_map.get(&hash).map(|x| *x)
                                } else {
                                    None
                                };
                                send_stream_id.and_then(|x| data.sender_ids_map.get(&x))
                            };
                            if let Some(send_stream) = send_stream {
                                let locked_send_stream = send_stream.lock();
                                if let Ok(send_stream) = locked_send_stream {
                                    let message= bidirectional_message.transaction_sig.map_or_else(
                                        || bidirectional_message.message_hash.map_or_else( || "".to_string(), |x| format!("hashed packet: {} message: {}", x, bidirectional_message.message).to_string()),
                                        |x| format!("transaction signature {}, message : {}", x, bidirectional_message.message));
                                    send_stream.write(message.as_bytes());
                                }
                            }
                        }
                    },
                    Err(e) => {
                        // recv error
                        warn!("got {} on quic bidirectional channel", e.to_string());
                        break;
                    }
                }
            }
        });
    }
}