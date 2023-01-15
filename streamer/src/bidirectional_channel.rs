// this is a service to handle bidirections quinn channel
use bincode::serialize;
use crossbeam_channel::{Receiver, Sender};
use quinn::SendStream;
use serde_derive::{Deserialize, Serialize};
use solana_perf::packet::{Packet, PacketBatch};
use solana_sdk::hash::Hash;
use solana_sdk::{message::Message, signature::Signature};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

#[derive(Serialize, Deserialize)]
pub struct QuicReplyMessage {
    transaction_sig: Option<Signature>,
    message_hash: Option<Hash>,
    message: String,
}

pub struct QuicBidirectionalData {
    pub message_hash_map: HashMap<Hash, u64>,
    pub transaction_signature_map: HashMap<Signature, u64>,
    pub sender_ids_map: HashMap<u64, Sender<QuicReplyMessage>>,
    pub sender_socket_address_map: HashMap<SocketAddr, u64>,
    pub last_id: u64,
}

#[derive(Clone)]
pub struct QuicBidirectionalReplyService {
    data: Arc<RwLock<QuicBidirectionalData>>,
    pub service_reciever: Receiver<QuicReplyMessage>,
    pub service_sender: Sender<QuicReplyMessage>,
}

pub fn get_signature_from_packet(packet: &Packet) -> Option<Signature> {
    // add instruction signature for message
    match packet.data(1..33) {
        Some(signature_bytes) => {
            let sig = Signature::new(&signature_bytes);
            Some(sig)
        }
        None => None,
    }
}

impl QuicBidirectionalReplyService {
    pub fn new() -> Self {
        let (service_sender, service_reciever) = crossbeam_channel::unbounded();
        Self {
            service_reciever: service_reciever,
            service_sender: service_sender,
            data: Arc::new(RwLock::new(QuicBidirectionalData {
                message_hash_map: HashMap::new(),
                transaction_signature_map: HashMap::new(),
                sender_ids_map: HashMap::new(),
                sender_socket_address_map: HashMap::new(),
                last_id: 1,
            })),
        }
    }

    pub fn add_stream(&self, quic_address: &SocketAddr, send_stream: SendStream) {
        let (sender_channel, reciever_channel) = crossbeam_channel::bounded(256);
        let data = self.data.write();
        let sender_id = if let Ok(mut data) = data {
            let data = &mut *data;
            if let Some(x) = data.sender_socket_address_map.get(quic_address) {
                // remove existing channel
                data.sender_ids_map.remove(x);
            };
            let current_id = data.last_id;
            data.last_id += 1;
            data.sender_ids_map
                .insert(current_id, sender_channel.clone());
            data.sender_socket_address_map
                .insert(quic_address.clone(), current_id);
            // create a new or replace the exisiting id
            data.sender_ids_map
                .insert(current_id, sender_channel.clone());
            data.sender_socket_address_map
                .insert(quic_address.clone(), current_id);
            current_id
        } else {
            0
        };

        if sender_id == 0 {
            return;
        }

        let subscriptions = self.data.clone();
        // start listnening to stream specific cross beam channel
        tokio::spawn(async move {
            tokio::pin!(send_stream);
            loop {
                let reciever_channel = reciever_channel.clone();
                let recv_task = async move { reciever_channel.recv() };
                let send_stream_close_task = async { send_stream.stopped() };
                tokio::select! {
                    task = recv_task => {
                        match task {
                            Ok(message) => {
                                let serialized_message = serialize(&message).unwrap();
                                let _ =send_stream.write(serialized_message.as_slice()).await;
                            },
                            Err(_) => {
                                // disconnect
                                let _ = send_stream.finish().await;
                                break;
                            }
                        }
                    },
                    _task = send_stream_close_task => {
                        // disconnect
                        let _ = send_stream.finish().await;
                        break;
                    }
                };
            }

            // remove all data belonging to sender_id
            let write_lock = subscriptions.write();
            if let Ok(mut sub_data) = write_lock {
                let subscriptions = &mut *sub_data;
                subscriptions
                    .message_hash_map
                    .retain(|_, v| *v != sender_id);
                subscriptions.sender_ids_map.remove(&sender_id);
                subscriptions
                    .transaction_signature_map
                    .retain(|_, v| *v != sender_id);
                subscriptions
                    .sender_socket_address_map
                    .retain(|_, v| *v != sender_id);
            }
        });
    }

    pub fn add_packets(&self, quic_address: &SocketAddr, packets: &PacketBatch) {
        // check if socket is registered;
        let id = {
            let data = self.data.read();
            if let Ok(data) = data {
                data.sender_socket_address_map
                    .get(quic_address)
                    .map_or(0, |x| *x)
            } else {
                0
            }
        };
        if id == 0 {
            return;
        }
        let data = self.data.write();
        if let Ok(mut data) = data {
            let data = &mut *data;
            packets.iter().for_each(|packet| {
                let signature = get_signature_from_packet(packet);
                signature.map(|x| data.transaction_signature_map.insert(x, id));

                if let Some(data_packet) = packet.data(..) {
                    let hash = Message::hash_raw_message(data_packet);
                    data.message_hash_map.insert(hash, id);
                }
            });
        }
    }


    // this method will start bidirectional relay service
    // the the message sent to bidirectional service, 
    // will be dispactched to the appropriate sender channel 
    // depending on transcation signature or message hash
    pub fn serve(&self) {
        let service_reciever = self.service_reciever.clone();
        let subscription_data = self.data.clone();
        tokio::spawn(async move {
            loop {
                let service_reciever = service_reciever.clone();
                let bidirectional_message = service_reciever.recv();
                match bidirectional_message {
                    Ok(bidirectional_message) => {
                        let data = subscription_data.read();

                        if let Ok(data) = data {
                            let send_stream = {
                                // if the message has transaction signature then find stream from transaction signature
                                // else find stream by packet hash
                                let send_stream_id =
                                    if let Some(sig) = bidirectional_message.transaction_sig {
                                        data.transaction_signature_map.get(&sig).map(|x| *x)
                                    } else if let Some(hash) = bidirectional_message.message_hash {
                                        data.message_hash_map.get(&hash).map(|x| *x)
                                    } else {
                                        None
                                    };
                                send_stream_id.and_then(|x| data.sender_ids_map.get(&x))
                            };
                            if let Some(send_stream) = send_stream {
                                match send_stream.send(bidirectional_message) {
                                    Err(e ) => {
                                        warn!("Error sending a bidirectional message {}", e.to_string());
                                    }
                                    _=>{}
                                }

                            }
                        }
                    }
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


#[cfg(test)]
pub mod test {
    use solana_perf::packet::Packet;
    use solana_sdk::{transaction::Transaction, system_instruction, signature::Keypair, signer::Signer, message::Message};

    use crate::bidirectional_channel::get_signature_from_packet;

    #[tokio::test]
    async fn test_we_correctly_get_signature_from_packet(){
        let k1 = Keypair::new();
        let k2 = Keypair::new();

        let hash = Message::hash_raw_message(&[0]);
        let ix = system_instruction::transfer(&k1.pubkey(), &k2.pubkey(), 10);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&k1.pubkey()), &[&k1], hash);
        let sig = tx.signatures[0];
        let packet = Packet::from_data(None, tx).unwrap();
        assert_eq!(Some(sig), get_signature_from_packet(&packet));
    }
}