// this is a service to handle bidirections quinn channel
use {
    bincode::serialize,
    quinn::SendStream,
    solana_perf::packet::{Packet, PacketBatch},
    solana_sdk::signature::Signature,
    std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc},
    tokio::{
        sync::{
            mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
            RwLock,
        },
        task::JoinHandle,
    },
};

pub struct QuicReplyMessage {
    transaction_signature: Signature,
    message: [u8; 128],
}

pub const QUIC_REPLY_MESSAGE_SIZE: usize = 8 + 64 + 128;
pub const QUIC_REPLY_MESSAGE_SIGNATURE_OFFSET: usize = 8;
pub const QUIC_REPLY_MESSAGE_OFFSET: usize = 8 + 64;

impl QuicReplyMessage {
    pub fn new(transaction_signature: Signature, message: String) -> Self {
        let message = message.as_bytes();
        let message: [u8; 128] = if message.len() >= 128 {
            message[..128].try_into().unwrap()
        } else {
            let mut array: [u8; 128] = [0; 128];
            message.iter().enumerate().for_each(|(index, char)| {
                array[index] = *char;
            });
            array
        };
        Self {
            message: message,
            transaction_signature: transaction_signature,
        }
    }

    pub fn new_with_bytes(transaction_signature: Signature, message: [u8; 128]) -> Self {
        Self {
            transaction_signature,
            message,
        }
    }

    pub fn message(&self) -> String {
        match String::from_utf8(self.message.to_vec()) {
            Ok(x) => x,
            Err(_) => String::from_str("").unwrap(),
        }
    }

    pub fn signature(&self) -> Signature {
        self.transaction_signature
    }
}

impl serde::Serialize for QuicReplyMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sig_bytes = match bincode::serialize(&self.transaction_signature) {
            Ok(bytes) => bytes,
            Err(_) => {
                // invalid signature / should not really happen
                [0; 64].to_vec()
            }
        };
        let message = &mut self.message.to_vec();
        sig_bytes.append(message);
        serializer.serialize_bytes(sig_bytes.as_slice())
    }
}

pub struct QuicBidirectionalData {
    pub transaction_signature_map: HashMap<Signature, u64>,
    pub sender_ids_map: HashMap<u64, UnboundedSender<QuicReplyMessage>>,
    pub sender_socket_address_map: HashMap<SocketAddr, u64>,
    pub last_id: u64,
}

#[derive(Clone)]
pub struct QuicBidirectionalReplyService {
    data: Arc<RwLock<QuicBidirectionalData>>,
    pub service_sender: UnboundedSender<QuicReplyMessage>,
    serving_handle: Arc<Option<JoinHandle<()>>>,
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
        let (service_sender, service_reciever) = unbounded_channel();
        let mut instance = Self {
            service_sender: service_sender,
            data: Arc::new(RwLock::new(QuicBidirectionalData {
                transaction_signature_map: HashMap::new(),
                sender_ids_map: HashMap::new(),
                sender_socket_address_map: HashMap::new(),
                last_id: 1,
            })),
            serving_handle: Arc::new(None),
        };
        let join_handle = instance.serve(service_reciever);
        instance.serving_handle = Arc::new(Some(join_handle));
        instance
    }

    pub fn send_message(&self, transaction_signature: &Signature, message: String) {
        println!("send_message {} {}", transaction_signature, message);
        let message = QuicReplyMessage::new(*transaction_signature, message);
        match self.service_sender.send(message) {
            Ok(_) => {
                println!("send ok");
            }
            Err(e) => {
                println!("send error {}", e);
            }
        }
    }

    // When we get a bidirectional stream then we add the send stream to the service.
    // This will create a crossbeam channel to dispatch messages and a task which will listen to the crossbeam channel and will send the replies back throught send stream.
    // When you add again a new send_stream channel for the same socket address then we will remove the previous one from the socket address map,
    // This will then destroy the sender of the crossbeam channel putting the reciever_channel in error state starting the clean up sequence for the old channel.
    // So when you add again a new channel for same socket, we will no longer get the messages for old channel.
    pub async fn add_stream(&self, quic_address: SocketAddr, send_stream: SendStream) {
        let (sender_channel, reciever_channel) = unbounded_channel();
        // context for writelocking the data
        let sender_id = {
            let mut data = self.data.write().await;
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

        println!("adding stream {}", sender_id);
        // start listnening to stream specific cross beam channel
        tokio::spawn(async move {
            let mut send_stream = send_stream;
            let mut reciever_channel = reciever_channel;
            loop {
                let recv_task = reciever_channel.recv();
                let finish = tokio::select! {
                    message = recv_task => {
                        if let Some(message) = message {
                            let serialized_message = serialize(&message).unwrap();

                            println!("packet batch for message {} of length {}", message.transaction_signature, serialized_message.len());
                            let res = send_stream.write_all(serialized_message.as_slice()).await;
                            if let Err(error) = res {
                                info!(
                                    "Bidirectional writing stopped for socket {} because {}",
                                    quic_address,
                                    error.to_string()
                                );
                                true
                            } else {
                                false
                            }
                        } else {
                            println!("recv channel closed");
                            true
                        }
                    },
                };

                if finish {
                    if finish {
                        println!("finishing the stream");
                        let _ = send_stream.finish().await;
                        break;
                    }
                }
            }
            println!("finshed stream {}", sender_id);
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
            println!("id is 0");
            return;
        }
        let mut data = self.data.write().await;
        let data = &mut *data;
        packets.iter().for_each(|packet| {
            let meta = &packet.meta;
            println!("got a packet in add_packet");
            if meta.discard()
                || meta.forwarded()
                || meta.is_simple_vote_tx()
                || meta.is_tracer_packet()
                || meta.repair()
            {
                return;
            }
            let signature = get_signature_from_packet(packet);

            println!("adding packet {} for id {}", signature.unwrap(), id);
            signature.map(|x| data.transaction_signature_map.insert(x, id));
        });

        println!("adding packets finished");
    }

    // this method will start bidirectional relay service
    // the the message sent to bidirectional service,
    // will be dispactched to the appropriate sender channel
    // depending on transcation signature or message hash
    pub fn serve(
        &self,
        service_reciever: UnboundedReceiver<QuicReplyMessage>,
    ) -> tokio::task::JoinHandle<()> {
        let subscription_data = self.data.clone();
        tokio::spawn(async move {
            let mut service_reciever = service_reciever;
            loop {
                println!("start serving");
                let bidirectional_message = service_reciever.recv().await;
                println!("serve recieved a message");
                if bidirectional_message.is_none() {
                    println!("bi dir channel closed");
                    // the channel has be closed
                    warn!("quic bidirectional channel is closed");
                    break;
                }
                let message = bidirectional_message.unwrap();

                let subscription_data = subscription_data.clone();

                let data = subscription_data.read().await;

                let send_stream = {
                    // if the message has transaction signature then find stream from transaction signature
                    // else find stream by packet hash
                    let send_stream_id = data
                        .transaction_signature_map
                        .get(&message.transaction_signature)
                        .map(|x| *x);
                    println!("serve for id {}", send_stream_id.unwrap());
                    send_stream_id.and_then(|x| data.sender_ids_map.get(&x))
                };
                if let Some(send_stream) = send_stream {
                    println!("sending message for {}", message.transaction_signature);
                    match send_stream.send(message) {
                        Err(e) => {
                            warn!("Error sending a bidirectional message {}", e.to_string());
                        }
                        _ => {}
                    }
                }
            }
        })
    }
}

#[cfg(test)]
pub mod test {
    use {
        crate::{
            bidirectional_channel::{get_signature_from_packet, QuicBidirectionalReplyService},
            nonblocking::quic::{spawn_server, test::get_client_config},
            quic::{StreamStats, MAX_UNSTAKED_CONNECTIONS},
            streamer::StakedNodes,
        },
        crossbeam_channel::unbounded,
        itertools::Itertools,
        quinn::{EndpointConfig, NewConnection},
        solana_client::{
            bidirectional_channel_handler::BidirectionalChannelHandler,
            connection_cache::ConnectionCacheStats,
            nonblocking::{
                quic_client::{QuicLazyInitializedEndpoint, QuicTpuConnection},
                tpu_connection::TpuConnection,
            },
        },
        solana_perf::packet::{Packet, PacketBatch},
        solana_sdk::{
            message::Message,
            signature::{Keypair, Signature},
            signer::Signer,
            system_instruction,
            transaction::{Transaction, TransactionError},
        },
        std::{
            net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
            sync::{atomic::AtomicBool, Arc, RwLock},
        },
        tokio::task::JoinHandle,
    };

    fn create_dummy_transaction(index: u8) -> Transaction {
        let k1 = Keypair::new();
        let k2 = Keypair::new();
        let hash = Message::hash_raw_message(&[index]);
        let ix = system_instruction::transfer(&k1.pubkey(), &k2.pubkey(), 10);
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&k1.pubkey()), &[&k1], hash);
        tx
    }

    fn create_n_transactions(size: usize) -> Vec<Transaction> {
        let mut ret = vec![];
        for i in 0..size {
            ret.push(create_dummy_transaction(i as u8));
        }
        ret
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
        );

        let t = match t {
            Ok(t) => t,
            Err(e) => {
                panic!("quic server error {}", e.to_string());
            }
        };
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

    pub fn create_a_quic_client(
        socket: SocketAddr,
        handler: BidirectionalChannelHandler,
    ) -> QuicTpuConnection {
        let mut connection_stats = ConnectionCacheStats::default();

        connection_stats.server_reply_channel = Some(handler);

        QuicTpuConnection::new(
            Arc::new(QuicLazyInitializedEndpoint::default()),
            socket,
            Arc::new(connection_stats),
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_addition_of_packets_with_quic_socket_registered() {
        let bidirectional_replay_service = QuicBidirectionalReplyService::new();

        let (_thread_handle, exit, reciever, server_address, _stream_stats) =
            setup_quic_server(bidirectional_replay_service.clone());

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let nb_packets = 5;

        // create transactions
        let transactions = create_n_transactions(5);
        let bidirectional_reply_handler = BidirectionalChannelHandler::new();
        let signatures = transactions.iter().map(|x| x.signatures[0]).collect_vec();

        // send transactions to the tpu
        let quic_client = create_a_quic_client(server_address, bidirectional_reply_handler.clone());
        let wire_transactions = transactions
            .iter()
            .map(|x| bincode::serialize(x).unwrap())
            .collect_vec();
        quic_client
            .send_wire_transaction_batch(&wire_transactions)
            .await
            .unwrap();

        // replying to each packet with a message
        let repling_thread = std::thread::spawn(move || loop {
            let mut i = 0;
            let packets = reciever.recv().unwrap();
            for packet in packets.iter() {
                let sig = get_signature_from_packet(&packet).unwrap();
                println!("getting reply for signature {} ", sig);
                bidirectional_replay_service
                    .send_message(&sig, TransactionError::InvalidRentPayingAccount.to_string());
                i += 1;
            }
            if i >= nb_packets {
                break;
            }
        });

        let sig_collector = std::thread::spawn(move || {
            let mut messages_to_return = vec![];
            loop {
                let mut i = 0;
                let message = bidirectional_reply_handler.reciever.recv().unwrap();
                println!("client for reply for signature {} ", message.signature());
                // check if transaction signature is present
                messages_to_return.push(message);
                i += 1;
                if i >= nb_packets {
                    break;
                }
            }
            messages_to_return
        });
        repling_thread.join().unwrap();
        let messages = sig_collector.join().unwrap();

        for message in messages {
            assert!(signatures.contains(&message.signature()));
            assert_eq!(
                message.message(),
                TransactionError::InvalidRentPayingAccount.to_string()
            );
        }
        exit.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
