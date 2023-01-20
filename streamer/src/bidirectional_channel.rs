// this is a service to handle bidirections quinn channel
use {
    bincode::serialize,
    quinn::SendStream,
    solana_perf::packet::{Packet, PacketBatch},
    solana_sdk::signature::Signature,
    std::{
        collections::HashMap,
        net::SocketAddr,
        str::FromStr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
        time::Duration,
    },
    tokio::{
        sync::{
            mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
            RwLock,
        },
        task::JoinHandle,
    },
    x509_parser::nom::AsBytes,
};

#[derive(Clone)]
pub struct QuicReplyMessage {
    transaction_signature: Signature,
    message: [u8; 128],
}
// header + signature + 128 bytes for message
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
            array[0..message.len()].copy_from_slice(message.as_bytes());
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
        let index_end = match self.message.iter().position(|x| *x == 0) {
            Some(x) => x,
            None => 128,
        };
        match String::from_utf8(self.message[0..index_end].to_vec()) {
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

const TRANSACTION_TIMEOUT: u64 = 30_000; // 30s

pub struct QuicBidirectionalData {
    pub transaction_signature_map: HashMap<Signature, Vec<u64>>,
    pub sender_ids_map: HashMap<u64, UnboundedSender<QuicReplyMessage>>,
    pub sender_socket_address_map: HashMap<SocketAddr, u64>,
    pub last_id: u64,
}

#[derive(Clone)]
pub struct QuicBidirectionalMetrics {
    pub connections_added: Arc<AtomicU64>,
    pub transactions_added: Arc<AtomicU64>,
    pub transactions_replied_to: Arc<AtomicU64>,
    pub transactions_removed: Arc<AtomicU64>,
    pub connections_disconnected: Arc<AtomicU64>,
}

impl QuicBidirectionalMetrics {
    pub fn new() -> Self {
        Self {
            connections_added: Arc::new(AtomicU64::new(0)),
            transactions_added: Arc::new(AtomicU64::new(0)),
            transactions_replied_to: Arc::new(AtomicU64::new(0)),
            connections_disconnected: Arc::new(AtomicU64::new(0)),
            transactions_removed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn connections_added(&self) -> u64 {
        self.connections_added.load(Ordering::Relaxed)
    }

    pub fn transactions_added(&self) -> u64 {
        self.transactions_added.load(Ordering::Relaxed)
    }

    pub fn transactions_replied_to(&self) -> u64 {
        self.transactions_replied_to.load(Ordering::Relaxed)
    }

    pub fn transactions_removed(&self) -> u64 {
        self.transactions_removed.load(Ordering::Relaxed)
    }

    pub fn connections_disconnected(&self) -> u64 {
        self.connections_disconnected.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct QuicBidirectionalReplyService {
    data: Arc<RwLock<QuicBidirectionalData>>,
    pub service_sender: UnboundedSender<QuicReplyMessage>,
    serving_handle: Arc<Option<JoinHandle<()>>>,
    pub metrics: QuicBidirectionalMetrics,
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
            metrics: QuicBidirectionalMetrics::new(),
        };
        let join_handle = instance.serve(service_reciever);
        instance.serving_handle = Arc::new(Some(join_handle));
        instance
    }

    pub fn send_message(&self, transaction_signature: &Signature, message: String) {
        let message = QuicReplyMessage::new(*transaction_signature, message);
        match self.service_sender.send(message) {
            Err(e) => {
                debug!("send error {}", e);
            }
            _ => {
                // continue
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

        self.metrics
            .connections_added
            .fetch_add(1, Ordering::Relaxed);
        let subscriptions = self.data.clone();
        let metrics = self.metrics.clone();

        // start listnening to stream specific cross beam channel
        tokio::spawn(async move {
            let mut send_stream = send_stream;
            let mut reciever_channel = reciever_channel;
            loop {
                let finish = tokio::select! {
                    message = reciever_channel.recv() => {
                        if let Some(message) = message {
                            let serialized_message = serialize(&message).unwrap();

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
                            trace!("recv channel closed");
                            true
                        }
                    },
                    _task = send_stream.stopped() => {
                        true
                    }
                };

                if finish {
                    trace!("finishing the stream");
                    let _ = send_stream.finish().await;
                    break;
                }
            }
            // remove all data belonging to sender_id
            let mut sub_data = subscriptions.write().await;
            let subscriptions = &mut *sub_data;
            subscriptions.sender_ids_map.remove(&sender_id);
            subscriptions
                .sender_socket_address_map
                .retain(|_, v| *v != sender_id);

            metrics
                .connections_disconnected
                .fetch_add(1, Ordering::Relaxed);
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
        let metrics = self.metrics.clone();
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
            metrics.transactions_added.fetch_add(1, Ordering::Relaxed);
            signature.map(|x| {
                let ids = data.transaction_signature_map.get_mut(&x);
                match ids {
                    Some(ids) => ids.push(id), // push in exisiting ids
                    None => {
                        data.transaction_signature_map.insert(x, vec![id]); // push a new id vector

                        // create a task to clean up Timedout transactions
                        let me = self.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(TRANSACTION_TIMEOUT)).await;
                            let mut data = me.data.write().await;
                            data.transaction_signature_map.remove(&x);
                            me.metrics
                                .transactions_removed
                                .fetch_add(1, Ordering::Relaxed);
                        });
                    }
                }
            });
        });
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
        let metrics = self.metrics.clone();
        tokio::spawn(async move {
            let mut service_reciever = service_reciever;
            let metrics = metrics.clone();
            loop {
                let bidirectional_message = service_reciever.recv().await;
                if bidirectional_message.is_none() {
                    // the channel has be closed
                    trace!("quic bidirectional channel is closed");
                    break;
                }
                let message = bidirectional_message.unwrap();

                let subscription_data = subscription_data.clone();

                let data = subscription_data.read().await;
                // if the message has transaction signature then find stream from transaction signature
                // else find stream by packet hash
                let send_stream_ids = data
                    .transaction_signature_map
                    .get(&message.transaction_signature)
                    .map(|x| x);
                if let Some(send_stream_ids) = send_stream_ids {
                    for send_stream_id in send_stream_ids {
                        if let Some(send_stream) = data.sender_ids_map.get(&send_stream_id) {
                            match send_stream.send(message.clone()) {
                                Err(e) => {
                                    warn!(
                                        "Error sending a bidirectional message {}",
                                        e.to_string()
                                    );
                                }
                                Ok(_) => {
                                    metrics
                                        .transactions_replied_to
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
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
            bidirectional_channel::{
                get_signature_from_packet, QuicBidirectionalReplyService, TRANSACTION_TIMEOUT,
            },
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
            time::Duration,
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

    // This test is the main test, we send 5 transactions to the quic connection and the quic connection replies
    // them with errors. Then we check ght the QuicReplyHandler process these messages correctly
    // we also check if the metrics are updated correctly, the connections are dropped  correctly
    // the transactions are removed after a timeout
    #[tokio::test]
    async fn test_send_5_transaction_to_quic_server_and_get_replies() {
        let bidirectional_replay_service = QuicBidirectionalReplyService::new();
        let metrics = bidirectional_replay_service.metrics.clone();

        let (_thread_handle, exit, reciever, server_address, _stream_stats) =
            setup_quic_server(bidirectional_replay_service.clone());
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

        assert_eq!(metrics.connections_added(), 1);
        // one shot channels so that we do not block runtime for joining threads
        let (oscs1, oscr1) = tokio::sync::oneshot::channel();
        let (oscs2, oscr2) = tokio::sync::oneshot::channel();

        // replying to each packet with a message
        std::thread::spawn(move || {
            let mut i = 0;
            loop {
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
            oscs1.send(()).unwrap();
        });

        std::thread::spawn(move || {
            let mut messages_to_return = vec![];
            let mut i = 0;
            loop {
                let message = bidirectional_reply_handler.reciever.recv().unwrap();
                // check if transaction signature is present
                messages_to_return.push(message);
                i += 1;
                if i >= nb_packets {
                    break;
                }
            }
            let _ = oscs2.send(messages_to_return);
        });
        oscr1.await.unwrap();
        let messages = oscr2.await.unwrap();

        // sleep some time so that other tasks can progress
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(metrics.transactions_added(), nb_packets);
        assert_eq!(metrics.transactions_replied_to(), nb_packets);
        assert_eq!(metrics.transactions_removed(), 0);
        // asserting for messages
        for message in messages {
            assert!(signatures.contains(&message.signature()));
            assert_eq!(
                message.message(),
                TransactionError::InvalidRentPayingAccount.to_string()
            );
        }
        assert_eq!(metrics.connections_disconnected(), 0);
        tokio::time::sleep(Duration::from_millis(TRANSACTION_TIMEOUT)).await;
        // connection is disconnected after the timeout
        assert_eq!(metrics.connections_added(), 1);
        assert_eq!(metrics.transactions_removed(), nb_packets);
        exit.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
