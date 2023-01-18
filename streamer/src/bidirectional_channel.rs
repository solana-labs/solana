// this is a service to handle bidirections quinn channel
use bincode::serialize;
use crossbeam_channel::{Receiver, Sender};
use quinn::SendStream;
use serde_derive::{Deserialize, Serialize};
use solana_perf::packet::{Packet, PacketBatch};
use solana_sdk::signature::Signature;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize)]
pub struct QuicReplyMessage {
    pub transaction_signature: Signature,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
pub struct QuicReplyMessageBatch {
    pub messages: Vec<QuicReplyMessage>,
}

pub struct QuicBidirectionalData {
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
    match packet.data(1..65) {
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
                transaction_signature_map: HashMap::new(),
                sender_ids_map: HashMap::new(),
                sender_socket_address_map: HashMap::new(),
                last_id: 1,
            })),
        }
    }

    pub fn send_message(&self, transaction_signature: &Signature, message: String) {
        let _ = self.service_sender.send(QuicReplyMessage {
            transaction_signature: *transaction_signature,
            message,
        });
    }

    // When we get a bidirectional stream then we add the send stream to the service.
    // This will create a crossbeam channel to dispatch messages and a task which will listen to the crossbeam channel and will send the replies back throught send stream.
    // When you add again a new send_stream channel for the same socket address then we will remove the previous one from the socket address map,
    // This will then destroy the sender of the crossbeam channel putting the reciever_channel in error state starting the clean up sequence for the old channel.
    // So when you add again a new channel for same socket, we will no longer get the messages for old channel.
    pub async fn add_stream(&self, quic_address: SocketAddr, send_stream: SendStream) {
        let (sender_channel, reciever_channel) = crossbeam_channel::unbounded();
        let mut data = self.data.write().await;
        let sender_id = {
            let data = &mut *data;
            if let Some(x) = data.sender_socket_address_map.get(&quic_address) {
                // remove existing channel and replace with new one
                // removing this channel should also destroy the p
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
        };

        let subscriptions = self.data.clone();
        // start listnening to stream specific cross beam channel
        tokio::spawn(async move {
            tokio::pin!(send_stream);
            loop {
                let reciever_channel = reciever_channel.clone();
                let recv_task = async move { reciever_channel.recv() };
                let send_stream_close_task = async { send_stream.stopped() };
                let mut message_batch = QuicReplyMessageBatch { messages: vec![] };

                let finish = tokio::select! {
                    task = recv_task => {
                        match task {
                            Ok(message) => {
                                message_batch.messages.push(message);
                                false
                            },
                            Err(_) => {
                                true
                            }
                        }
                    },
                    _task = send_stream_close_task => {
                        true
                    }
                };
                // limiting t
                if finish || message_batch.messages.len() >= 16 {
                    let serialized_message = serialize(&message_batch).unwrap();
                    let res = send_stream.write_all(serialized_message.as_slice()).await;
                    if let Err(error) = res {
                        info!(
                            "Bidirectional writing stopped for socket {} because {}",
                            quic_address,
                            error.to_string()
                        );
                        break;
                    }
                    if finish {
                        let _ = send_stream.finish().await;
                        break;
                    }
                }
            }

            // remove all data belonging to sender_id
            let mut sub_data = subscriptions.write().await;
            let subscriptions = &mut *sub_data;
            subscriptions.sender_ids_map.remove(&sender_id);
            subscriptions
                .transaction_signature_map
                .retain(|_, v| *v != sender_id);
            subscriptions
                .sender_socket_address_map
                .retain(|_, v| *v != sender_id);
        });
    }

    pub async fn add_packets(&self, quic_address: &SocketAddr, packets: &PacketBatch) {
        // check if socket is registered;
        let id = {
            let data = self.data.read().await;
            data.sender_socket_address_map
                .get(quic_address)
                .map_or(0, |x| *x)
        };

        // this means that there is not bidirectional connection, and packets came from unidirectional socket
        if id == 0 {
            return;
        }
        let mut data = self.data.write().await;
        let data = &mut *data;
        packets.iter().for_each(|packet| {
            let meta = &packet.meta;
            if meta.discard()
                || meta.forwarded()
                || meta.is_simple_vote_tx()
                || meta.is_tracer_packet()
                || meta.repair()
            {
                return;
            }

            let signature = get_signature_from_packet(packet);
            signature.map(|x| data.transaction_signature_map.insert(x, id));
        });
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
                        let data = subscription_data.read().await;

                        let send_stream = {
                            // if the message has transaction signature then find stream from transaction signature
                            // else find stream by packet hash
                            let send_stream_id = data
                                .transaction_signature_map
                                .get(&bidirectional_message.transaction_signature)
                                .map(|x| *x);
                            send_stream_id.and_then(|x| data.sender_ids_map.get(&x))
                        };
                        if let Some(send_stream) = send_stream {
                            match send_stream.send(bidirectional_message) {
                                Err(e) => {
                                    warn!(
                                        "Error sending a bidirectional message {}",
                                        e.to_string()
                                    );
                                }
                                _ => {}
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
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::{Arc, RwLock};

    use itertools::Itertools;
    use quinn::{EndpointConfig, NewConnection};
    use solana_client::bidirectional_channel_handler::BidirectionalChannelHandler;

    use crate::nonblocking::quic::{spawn_server, test::get_client_config};
    use crate::quic::{StreamStats, MAX_UNSTAKED_CONNECTIONS};
    use crate::streamer::StakedNodes;
    use crossbeam_channel::unbounded;
    use solana_perf::packet::{Packet, PacketBatch};
    use solana_sdk::transaction::TransactionError;
    use solana_sdk::{
        message::Message,
        signature::{Keypair, Signature},
        signer::Signer,
        system_instruction,
        transaction::Transaction,
    };
    use std::net::UdpSocket;
    use std::sync::atomic::AtomicBool;
    use tokio::task::JoinHandle;

    use crate::bidirectional_channel::{get_signature_from_packet, QuicBidirectionalReplyService};

    fn _create_dummy_transaction() -> Transaction {
        let k1 = Keypair::new();
        let k2 = Keypair::new();
        let hash = Message::hash_raw_message(&[0]);
        let ix = system_instruction::transfer(&k1.pubkey(), &k2.pubkey(), 10);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&k1.pubkey()), &[&k1], hash);
        tx
    }

    fn _create_n_transactions(size: usize) -> Vec<Transaction> {
        vec![_create_dummy_transaction(); size]
    }

    fn create_dummy_packet() -> (Packet, Signature) {
        let k1 = Keypair::new();
        let k2 = Keypair::new();

        let hash = Message::hash_raw_message(&[0]);
        let ix = system_instruction::transfer(&k1.pubkey(), &k2.pubkey(), 10);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&k1.pubkey()), &[&k1], hash);
        let sig = tx.signatures[0];
        (Packet::from_data(None, tx).unwrap(), sig)
    }

    fn create_dummy_packet_batch(size: usize) -> PacketBatch {
        let mut vec = vec![];
        for _i in 0..size {
            vec.push(create_dummy_packet().0)
        }
        PacketBatch::new(vec)
    }

    #[tokio::test]
    async fn test_we_correctly_get_signature_from_packet() {
        let (packet, sig) = create_dummy_packet();
        assert_eq!(Some(sig), get_signature_from_packet(&packet));
    }

    #[tokio::test]
    async fn test_addition_add_packets_without_any_quic_socket_registered() {
        let bidirectional_replay_service = QuicBidirectionalReplyService::new();

        bidirectional_replay_service.serve();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 20000);
        let batch = create_dummy_packet_batch(5);
        bidirectional_replay_service
            .add_packets(&socket, &batch)
            .await;
    }

    fn setup_quic_server(
        bidirectional_quic_service: QuicBidirectionalReplyService,
    ) -> (
        JoinHandle<()>,
        Arc<AtomicBool>,
        crossbeam_channel::Receiver<PacketBatch>,
        SocketAddr,
        Arc<StreamStats>,
    ) {
        let option_staked_nodes: Option<StakedNodes> = None;
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(option_staked_nodes.unwrap_or_default()));
        let stats = Arc::new(StreamStats::default());

        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_UNSTAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats.clone(),
            bidirectional_quic_service,
        )
        .unwrap();
        (t, exit, receiver, server_address, stats)
    }

    pub async fn make_bidirectional_client_endpoint(addr: &SocketAddr) -> NewConnection {
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, client_socket)
            .unwrap()
            .0;
        let default_keypair = Keypair::new();
        endpoint.set_default_client_config(get_client_config(&default_keypair));
        endpoint
            .connect(*addr, "localhost")
            .expect("Failed in connecting")
            .await
            .expect("Failed in waiting")
    }

    pub async fn send_packet_batch(
        conn: Arc<NewConnection>,
        packets: &PacketBatch,
        bidirectional_reply_handler: BidirectionalChannelHandler,
    ) {
        // Send a full size packet with single byte writes.
        let (mut s1, recv) = conn.connection.open_bi().await.unwrap();
        bidirectional_reply_handler.start_serving(recv);
        for i in 0..packets.len() {
            s1.write_all(packets[i].data(..).unwrap()).await.unwrap();
        }
        s1.finish().await.unwrap();
    }

    #[tokio::test]
    async fn test_addition_of_packets_with_quic_socket_registered() {
        let bidirectional_replay_service = QuicBidirectionalReplyService::new();
        bidirectional_replay_service.serve();

        let bidirectional_reply_handler = BidirectionalChannelHandler::new();

        let (_thread_handle, exit, reciever, server_address, _stream_stats) =
            setup_quic_server(bidirectional_replay_service.clone());

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let nb_packets = 5;

        let conn = Arc::new(make_bidirectional_client_endpoint(&server_address).await);
        let batch = create_dummy_packet_batch(nb_packets);
        let signatures = batch
            .iter()
            .map(|x| get_signature_from_packet(x).unwrap())
            .collect_vec();
        send_packet_batch(conn, &batch, bidirectional_reply_handler.clone()).await;

        // replying to each packet with a message
        loop {
            let mut i = 0;
            let packets = reciever.recv().unwrap();
            for packet in packets.iter() {
                let sig = get_signature_from_packet(&packet).unwrap();
                bidirectional_replay_service
                    .send_message(&sig, TransactionError::InvalidRentPayingAccount.to_string());
                i += 1;
            }
            if i >= nb_packets {
                break;
            }
        }

        loop {
            let mut i = 0;
            let messages = bidirectional_reply_handler.reciever.recv().unwrap();
            for message in messages.messages {
                // check if transaction signature is present
                assert!(signatures.contains(&message.transaction_signature));
                assert_eq!(
                    message.message,
                    TransactionError::InvalidRentPayingAccount.to_string()
                );
                i += 1;
            }
            if i >= nb_packets {
                break;
            }
        }
        exit.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
