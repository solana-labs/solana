//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use accountant::Accountant;
use accounting_stage::AccountingStage;
use bincode::{deserialize, serialize, serialize_into};
use crdt::{Crdt, ReplicatedData};
use ecdsa;
use entry::Entry;
use event::Event;
use packet;
use packet::{SharedBlob, SharedPackets, BLOB_SIZE};
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use result::Result;
use serde_json;
use std::collections::VecDeque;
use std::io::sink;
use std::io::{Cursor, Write};
use std::mem::size_of;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use streamer;
use thin_client_service::{Request, Response, ThinClientService};
use timing;

pub struct Tpu {
    accounting_stage: AccountingStage,
    thin_client_service: ThinClientService,
}

type SharedTpu = Arc<Tpu>;

impl Tpu {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(accounting_stage: AccountingStage) -> Self {
        let thin_client_service = ThinClientService::new(accounting_stage.acc.clone());
        Tpu {
            accounting_stage,
            thin_client_service,
        }
    }

    fn update_entry<W: Write>(obj: &Tpu, writer: &Mutex<W>, entry: &Entry) {
        trace!("update_entry entry");
        obj.accounting_stage.acc.register_entry_id(&entry.id);
        writeln!(
            writer.lock().unwrap(),
            "{}",
            serde_json::to_string(&entry).unwrap()
        ).unwrap();
        obj.thin_client_service
            .notify_entry_info_subscribers(&entry);
    }

    fn receive_all<W: Write>(obj: &Tpu, writer: &Mutex<W>) -> Result<Vec<Entry>> {
        //TODO implement a serialize for channel that does this without allocations
        let mut l = vec![];
        let entry = obj.accounting_stage
            .output
            .lock()
            .unwrap()
            .recv_timeout(Duration::new(1, 0))?;
        Self::update_entry(obj, writer, &entry);
        l.push(entry);
        while let Ok(entry) = obj.accounting_stage.output.lock().unwrap().try_recv() {
            Self::update_entry(obj, writer, &entry);
            l.push(entry);
        }
        Ok(l)
    }

    fn process_entry_list_into_blobs(
        list: &Vec<Entry>,
        blob_recycler: &packet::BlobRecycler,
        q: &mut VecDeque<SharedBlob>,
    ) {
        let mut start = 0;
        let mut end = 0;
        while start < list.len() {
            let mut total = 0;
            for i in &list[start..] {
                total += size_of::<Event>() * i.events.len();
                total += size_of::<Entry>();
                if total >= BLOB_SIZE {
                    break;
                }
                end += 1;
            }
            // See that we made progress and a single
            // vec of Events wasn't too big for a single packet
            if end <= start {
                // Trust the recorder to not package more than we can
                // serialize
                end += 1;
            }

            let b = blob_recycler.allocate();
            let pos = {
                let mut bd = b.write().unwrap();
                let mut out = Cursor::new(bd.data_mut());
                serialize_into(&mut out, &list[start..end]).expect("failed to serialize output");
                out.position() as usize
            };
            assert!(pos < BLOB_SIZE);
            b.write().unwrap().set_size(pos);
            q.push_back(b);
            start = end;
        }
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    fn run_sync<W: Write>(
        obj: SharedTpu,
        broadcast: &streamer::BlobSender,
        blob_recycler: &packet::BlobRecycler,
        writer: &Mutex<W>,
    ) -> Result<()> {
        let mut q = VecDeque::new();
        let list = Self::receive_all(&obj, writer)?;
        trace!("New blobs? {}", list.len());
        Self::process_entry_list_into_blobs(&list, blob_recycler, &mut q);
        if !q.is_empty() {
            broadcast.send(q)?;
        }
        Ok(())
    }

    pub fn sync_service<W: Write + Send + 'static>(
        obj: SharedTpu,
        exit: Arc<AtomicBool>,
        broadcast: streamer::BlobSender,
        blob_recycler: packet::BlobRecycler,
        writer: Mutex<W>,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            let _ = Self::run_sync(obj.clone(), &broadcast, &blob_recycler, &writer);
            if exit.load(Ordering::Relaxed) {
                info!("sync_service exiting");
                break;
            }
        })
    }

    fn process_thin_client_requests(_acc: &Arc<Accountant>, _socket: &UdpSocket) -> Result<()> {
        Ok(())
    }

    fn thin_client_service(
        acc: Arc<Accountant>,
        exit: Arc<AtomicBool>,
        socket: UdpSocket,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            let _ = Self::process_thin_client_requests(&acc, &socket);
            if exit.load(Ordering::Relaxed) {
                info!("sync_service exiting");
                break;
            }
        })
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    fn run_sync_no_broadcast(obj: SharedTpu) -> Result<()> {
        Self::receive_all(&obj, &Arc::new(Mutex::new(sink())))?;
        Ok(())
    }

    pub fn sync_no_broadcast_service(obj: SharedTpu, exit: Arc<AtomicBool>) -> JoinHandle<()> {
        spawn(move || loop {
            let _ = Self::run_sync_no_broadcast(obj.clone());
            if exit.load(Ordering::Relaxed) {
                info!("sync_no_broadcast_service exiting");
                break;
            }
        })
    }

    fn recv_batch(recvr: &streamer::PacketReceiver) -> Result<(Vec<SharedPackets>, usize)> {
        let timer = Duration::new(1, 0);
        let msgs = recvr.recv_timeout(timer)?;
        debug!("got msgs");
        let mut len = msgs.read().unwrap().packets.len();
        let mut batch = vec![msgs];
        while let Ok(more) = recvr.try_recv() {
            trace!("got more msgs");
            len += more.read().unwrap().packets.len();
            batch.push(more);

            if len > 100_000 {
                break;
            }
        }
        debug!("batch len {}", batch.len());
        Ok((batch, len))
    }

    fn verify_batch(
        batch: Vec<SharedPackets>,
        sendr: &Arc<Mutex<Sender<Vec<(SharedPackets, Vec<u8>)>>>>,
    ) -> Result<()> {
        let r = ecdsa::ed25519_verify(&batch);
        let res = batch.into_iter().zip(r).collect();
        sendr.lock().unwrap().send(res)?;
        // TODO: fix error handling here?
        Ok(())
    }

    fn verifier(
        recvr: &Arc<Mutex<streamer::PacketReceiver>>,
        sendr: &Arc<Mutex<Sender<Vec<(SharedPackets, Vec<u8>)>>>>,
    ) -> Result<()> {
        let (batch, len) = Self::recv_batch(&recvr.lock().unwrap())?;
        let now = Instant::now();
        let batch_len = batch.len();
        let rand_id = thread_rng().gen_range(0, 100);
        info!(
            "@{:?} verifier: verifying: {} id: {}",
            timing::timestamp(),
            batch.len(),
            rand_id
        );

        Self::verify_batch(batch, sendr).unwrap();

        let total_time_ms = timing::duration_as_ms(&now.elapsed());
        let total_time_s = timing::duration_as_s(&now.elapsed());
        info!(
            "@{:?} verifier: done. batches: {} total verify time: {:?} id: {} verified: {} v/s {}",
            timing::timestamp(),
            batch_len,
            total_time_ms,
            rand_id,
            len,
            (len as f32 / total_time_s)
        );
        Ok(())
    }

    pub fn deserialize_packets(p: &packet::Packets) -> Vec<Option<(Request, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            })
            .collect()
    }

    /// Split Request list into verified transactions and the rest
    fn partition_requests(
        req_vers: Vec<(Request, SocketAddr, u8)>,
    ) -> (Vec<Event>, Vec<(Request, SocketAddr)>) {
        let mut events = vec![];
        let mut reqs = vec![];
        for (msg, rsp_addr, verify) in req_vers {
            match msg {
                Request::Transaction(tr) => {
                    if verify != 0 {
                        events.push(Event::Transaction(tr));
                    }
                }
                _ => reqs.push((msg, rsp_addr)),
            }
        }
        (events, reqs)
    }

    fn serialize_response(
        resp: Response,
        rsp_addr: SocketAddr,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<packet::SharedBlob> {
        let blob = blob_recycler.allocate();
        {
            let mut b = blob.write().unwrap();
            let v = serialize(&resp)?;
            let len = v.len();
            b.data[..len].copy_from_slice(&v);
            b.meta.size = len;
            b.meta.set_addr(&rsp_addr);
        }
        Ok(blob)
    }

    fn serialize_responses(
        rsps: Vec<(Response, SocketAddr)>,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<VecDeque<packet::SharedBlob>> {
        let mut blobs = VecDeque::new();
        for (resp, rsp_addr) in rsps {
            blobs.push_back(Self::serialize_response(resp, rsp_addr, blob_recycler)?);
        }
        Ok(blobs)
    }

    fn process(
        obj: &Tpu,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        responder_sender: &streamer::BlobSender,
        packet_recycler: &packet::PacketRecycler,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let recv_start = Instant::now();
        let mms = verified_receiver.recv_timeout(timer)?;
        let mut reqs_len = 0;
        let mms_len = mms.len();
        info!(
            "@{:?} process start stalled for: {:?}ms batches: {}",
            timing::timestamp(),
            timing::duration_as_ms(&recv_start.elapsed()),
            mms.len(),
        );
        let proc_start = Instant::now();
        for (msgs, vers) in mms {
            let reqs = Self::deserialize_packets(&msgs.read().unwrap());
            reqs_len += reqs.len();
            let req_vers = reqs.into_iter()
                .zip(vers)
                .filter_map(|(req, ver)| req.map(|(msg, addr)| (msg, addr, ver)))
                .filter(|x| {
                    let v = x.0.verify();
                    v
                })
                .collect();

            debug!("partitioning");
            let (events, reqs) = Self::partition_requests(req_vers);
            debug!("events: {} reqs: {}", events.len(), reqs.len());

            debug!("process_events");
            obj.accounting_stage.process_events(events)?;
            debug!("done process_events");

            debug!("process_requests");
            let rsps = obj.thin_client_service.process_requests(reqs);
            debug!("done process_requests");

            let blobs = Self::serialize_responses(rsps, blob_recycler)?;
            if !blobs.is_empty() {
                info!("process: sending blobs: {}", blobs.len());
                //don't wake up the other side if there is nothing
                responder_sender.send(blobs)?;
            }
            packet_recycler.recycle(msgs);
        }
        let total_time_s = timing::duration_as_s(&proc_start.elapsed());
        let total_time_ms = timing::duration_as_ms(&proc_start.elapsed());
        info!(
            "@{:?} done process batches: {} time: {:?}ms reqs: {} reqs/s: {}",
            timing::timestamp(),
            mms_len,
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        Ok(())
    }
    /// Process verified blobs, already in order
    /// Respond with a signed hash of the state
    fn replicate_state(
        obj: &Tpu,
        verified_receiver: &streamer::BlobReceiver,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = verified_receiver.recv_timeout(timer)?;
        trace!("replicating blobs {}", blobs.len());
        for msgs in &blobs {
            let blob = msgs.read().unwrap();
            let entries: Vec<Entry> = deserialize(&blob.data()[..blob.meta.size]).unwrap();
            let acc = &obj.accounting_stage.acc;
            for entry in entries {
                acc.register_entry_id(&entry.id);
                for result in acc.process_verified_events(entry.events) {
                    result?;
                }
            }
            //TODO respond back to leader with hash of the state
        }
        for blob in blobs {
            blob_recycler.recycle(blob);
        }
        Ok(())
    }

    /// Create a UDP microservice that forwards messages the given Tpu.
    /// This service is the network leader
    /// Set `exit` to shutdown its threads.
    pub fn serve<W: Write + Send + 'static>(
        obj: &SharedTpu,
        me: ReplicatedData,
        serve: UdpSocket,
        skinny: UdpSocket,
        gossip: UdpSocket,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Result<Vec<JoinHandle<()>>> {
        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        let t_gossip = Crdt::gossip(crdt.clone(), exit.clone());
        let t_listen = Crdt::listen(crdt.clone(), gossip, exit.clone());

        // make sure we are on the same interface
        let mut local = serve.local_addr()?;
        local.set_port(0);
        let respond_socket = UdpSocket::bind(local.clone())?;

        let packet_recycler = packet::PacketRecycler::default();
        let blob_recycler = packet::BlobRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver =
            streamer::receiver(serve, exit.clone(), packet_recycler.clone(), packet_sender)?;
        let (responder_sender, responder_receiver) = channel();
        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            responder_receiver,
        );
        let (verified_sender, verified_receiver) = channel();

        let mut verify_threads = Vec::new();
        let shared_verified_sender = Arc::new(Mutex::new(verified_sender));
        let shared_packet_receiver = Arc::new(Mutex::new(packet_receiver));
        for _ in 0..4 {
            let exit_ = exit.clone();
            let recv = shared_packet_receiver.clone();
            let sender = shared_verified_sender.clone();
            let thread = spawn(move || loop {
                let e = Self::verifier(&recv, &sender);
                if e.is_err() && exit_.load(Ordering::Relaxed) {
                    break;
                }
            });
            verify_threads.push(thread);
        }

        let (broadcast_sender, broadcast_receiver) = channel();

        let broadcast_socket = UdpSocket::bind(local)?;
        let t_broadcast = streamer::broadcaster(
            broadcast_socket,
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            broadcast_receiver,
        );

        let t_sync = Self::sync_service(
            obj.clone(),
            exit.clone(),
            broadcast_sender,
            blob_recycler.clone(),
            Mutex::new(writer),
        );

        let t_skinny =
            Self::thin_client_service(obj.accounting_stage.acc.clone(), exit.clone(), skinny);

        let tpu = obj.clone();
        let t_server = spawn(move || loop {
            let e = Self::process(
                &mut tpu.clone(),
                &verified_receiver,
                &responder_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        let mut threads = vec![
            t_receiver,
            t_responder,
            t_server,
            t_sync,
            t_skinny,
            t_gossip,
            t_listen,
            t_broadcast,
        ];
        threads.extend(verify_threads.into_iter());
        Ok(threads)
    }

    /// This service receives messages from a leader in the network and processes the transactions
    /// on the accountant state.
    /// # Arguments
    /// * `obj` - The accountant state.
    /// * `me` - my configuration
    /// * `leader` - leader configuration
    /// * `exit` - The exit signal.
    /// # Remarks
    /// The pipeline is constructed as follows:
    /// 1. receive blobs from the network, these are out of order
    /// 2. verify blobs, PoH, signatures (TODO)
    /// 3. reconstruct contiguous window
    ///     a. order the blobs
    ///     b. use erasure coding to reconstruct missing blobs
    ///     c. ask the network for missing blobs, if erasure coding is insufficient
    ///     d. make sure that the blobs PoH sequences connect (TODO)
    /// 4. process the transaction state machine
    /// 5. respond with the hash of the state back to the leader
    pub fn replicate(
        obj: &SharedTpu,
        me: ReplicatedData,
        gossip: UdpSocket,
        serve: UdpSocket,
        replicate: UdpSocket,
        leader: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Result<Vec<JoinHandle<()>>> {
        //replicate pipeline
        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        crdt.write().unwrap().set_leader(leader.id);
        crdt.write().unwrap().insert(leader);
        let t_gossip = Crdt::gossip(crdt.clone(), exit.clone());
        let t_listen = Crdt::listen(crdt.clone(), gossip, exit.clone());

        // make sure we are on the same interface
        let mut local = replicate.local_addr()?;
        local.set_port(0);
        let write = UdpSocket::bind(local)?;

        let blob_recycler = packet::BlobRecycler::default();
        let (blob_sender, blob_receiver) = channel();
        let t_blob_receiver = streamer::blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            replicate,
            blob_sender.clone(),
        )?;
        let (window_sender, window_receiver) = channel();
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = streamer::retransmitter(
            write,
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            retransmit_receiver,
        );

        //TODO
        //the packets coming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let t_window = streamer::window(
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            blob_receiver,
            window_sender,
            retransmit_sender,
        );

        let tpu = obj.clone();
        let s_exit = exit.clone();
        let t_replicator = spawn(move || loop {
            let e = Self::replicate_state(&tpu, &window_receiver, &blob_recycler);
            if e.is_err() && s_exit.load(Ordering::Relaxed) {
                break;
            }
        });

        //serve pipeline
        // make sure we are on the same interface
        let mut local = serve.local_addr()?;
        local.set_port(0);
        let respond_socket = UdpSocket::bind(local.clone())?;

        let packet_recycler = packet::PacketRecycler::default();
        let blob_recycler = packet::BlobRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_packet_receiver =
            streamer::receiver(serve, exit.clone(), packet_recycler.clone(), packet_sender)?;
        let (responder_sender, responder_receiver) = channel();
        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            responder_receiver,
        );
        let (verified_sender, verified_receiver) = channel();

        let mut verify_threads = Vec::new();
        let shared_verified_sender = Arc::new(Mutex::new(verified_sender));
        let shared_packet_receiver = Arc::new(Mutex::new(packet_receiver));
        for _ in 0..4 {
            let exit_ = exit.clone();
            let recv = shared_packet_receiver.clone();
            let sender = shared_verified_sender.clone();
            let thread = spawn(move || loop {
                let e = Self::verifier(&recv, &sender);
                if e.is_err() && exit_.load(Ordering::Relaxed) {
                    break;
                }
            });
            verify_threads.push(thread);
        }
        let t_sync = Self::sync_no_broadcast_service(obj.clone(), exit.clone());

        let tpu = obj.clone();
        let s_exit = exit.clone();
        let t_server = spawn(move || loop {
            let e = Self::process(
                &mut tpu.clone(),
                &verified_receiver,
                &responder_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() {
                if s_exit.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        let mut threads = vec![
            //replicate threads
            t_blob_receiver,
            t_retransmit,
            t_window,
            t_replicator,
            t_gossip,
            t_listen,
            //serve threads
            t_packet_receiver,
            t_responder,
            t_server,
            t_sync,
        ];
        threads.extend(verify_threads.into_iter());
        Ok(threads)
    }
}

#[cfg(test)]
pub fn to_packets(r: &packet::PacketRecycler, reqs: Vec<Request>) -> Vec<SharedPackets> {
    let mut out = vec![];
    for rrs in reqs.chunks(packet::NUM_PACKETS) {
        let p = r.allocate();
        p.write()
            .unwrap()
            .packets
            .resize(rrs.len(), Default::default());
        for (i, o) in rrs.iter().zip(p.write().unwrap().packets.iter_mut()) {
            let v = serialize(&i).expect("serialize request");
            let len = v.len();
            o.data[..len].copy_from_slice(&v);
            o.meta.size = len;
        }
        out.push(p);
    }
    return out;
}

#[cfg(test)]
pub fn test_node() -> (ReplicatedData, UdpSocket, UdpSocket, UdpSocket, UdpSocket) {
    use signature::{KeyPair, KeyPairUtil};

    let skinny = UdpSocket::bind("127.0.0.1:0").unwrap();
    let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
    let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
    let serve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let pubkey = KeyPair::new().pubkey();
    let d = ReplicatedData::new(
        pubkey,
        gossip.local_addr().unwrap(),
        replicate.local_addr().unwrap(),
        serve.local_addr().unwrap(),
    );
    (d, gossip, replicate, serve, skinny)
}

#[cfg(test)]
mod tests {
    use accountant::Accountant;
    use accounting_stage::AccountingStage;
    use bincode::serialize;
    use chrono::prelude::*;
    use crdt::Crdt;
    use ecdsa;
    use entry;
    use event::Event;
    use hash::{hash, Hash};
    use logger;
    use mint::Mint;
    use packet::{BlobRecycler, PacketRecycler, BLOB_SIZE, NUM_PACKETS};
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer;
    use tpu::{test_node, to_packets, Request, Tpu};
    use transaction::{memfind, test_tx, Transaction};

    #[test]
    fn test_layout() {
        let tr = test_tx();
        let tx = serialize(&tr).unwrap();
        let packet = serialize(&Request::Transaction(tr)).unwrap();
        assert_matches!(memfind(&packet, &tx), Some(ecdsa::TX_OFFSET));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }
    #[test]
    fn test_to_packets() {
        let tr = Request::Transaction(test_tx());
        let re = PacketRecycler::default();
        let rv = to_packets(&re, vec![tr.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), 1);

        let rv = to_packets(&re, vec![tr.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);

        let rv = to_packets(&re, vec![tr.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].read().unwrap().packets.len(), 1);
    }

    /// Test that mesasge sent from leader to target1 and repliated to target2
    #[test]
    #[ignore]
    fn test_replicate() {
        logger::setup();
        let (leader_data, leader_gossip, _, leader_serve, _) = test_node();
        let (target1_data, target1_gossip, target1_replicate, target1_serve, _) = test_node();
        let (target2_data, target2_gossip, target2_replicate, _, _) = test_node();
        let exit = Arc::new(AtomicBool::new(false));

        //start crdt_leader
        let mut crdt_l = Crdt::new(leader_data.clone());
        crdt_l.set_leader(leader_data.id);

        let cref_l = Arc::new(RwLock::new(crdt_l));
        let t_l_gossip = Crdt::gossip(cref_l.clone(), exit.clone());
        let t_l_listen = Crdt::listen(cref_l, leader_gossip, exit.clone());

        //start crdt2
        let mut crdt2 = Crdt::new(target2_data.clone());
        crdt2.insert(leader_data.clone());
        crdt2.set_leader(leader_data.id);
        let leader_id = leader_data.id;
        let cref2 = Arc::new(RwLock::new(crdt2));
        let t2_gossip = Crdt::gossip(cref2.clone(), exit.clone());
        let t2_listen = Crdt::listen(cref2, target2_gossip, exit.clone());

        // setup some blob services to send blobs into the socket
        // to simulate the source peer and get blobs out of the socket to
        // simulate target peer
        let recv_recycler = BlobRecycler::default();
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = streamer::blob_receiver(
            exit.clone(),
            recv_recycler.clone(),
            target2_replicate,
            s_reader,
        ).unwrap();

        // simulate leader sending messages
        let (s_responder, r_responder) = channel();
        let t_responder = streamer::responder(
            leader_serve,
            exit.clone(),
            resp_recycler.clone(),
            r_responder,
        );

        let starting_balance = 10_000;
        let alice = Mint::new(starting_balance);
        let acc = Accountant::new(&alice);
        let accounting_stage = AccountingStage::new(acc, &alice.last_id(), Some(30));
        let tpu = Arc::new(Tpu::new(accounting_stage));
        let replicate_addr = target1_data.replicate_addr;
        let threads = Tpu::replicate(
            &tpu,
            target1_data,
            target1_gossip,
            target1_serve,
            target1_replicate,
            leader_data,
            exit.clone(),
        ).unwrap();

        let mut alice_ref_balance = starting_balance;
        let mut msgs = VecDeque::new();
        let mut cur_hash = Hash::default();
        let num_blobs = 10;
        let transfer_amount = 501;
        let bob_keypair = KeyPair::new();
        for i in 0..num_blobs {
            let b = resp_recycler.allocate();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            w.set_index(i).unwrap();
            w.set_id(leader_id).unwrap();

            let acc = &tpu.accounting_stage.acc;

            let tr0 = Event::new_timestamp(&bob_keypair, Utc::now());
            let entry0 = entry::create_entry(&cur_hash, i, vec![tr0]);
            acc.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            let tr1 = Transaction::new(
                &alice.keypair(),
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
            );
            acc.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);
            let entry1 =
                entry::create_entry(&cur_hash, i + num_blobs, vec![Event::Transaction(tr1)]);
            acc.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            alice_ref_balance -= transfer_amount;

            let serialized_entry = serialize(&vec![entry0, entry1]).unwrap();

            w.data_mut()[..serialized_entry.len()].copy_from_slice(&serialized_entry);
            w.set_size(serialized_entry.len());
            w.meta.set_addr(&replicate_addr);
            drop(w);
            msgs.push_back(b_);
        }

        // send the blobs into the socket
        s_responder.send(msgs).expect("send");

        // receive retransmitted messages
        let timer = Duration::new(1, 0);
        let mut msgs: Vec<_> = Vec::new();
        while let Ok(msg) = r_reader.recv_timeout(timer) {
            trace!("msg: {:?}", msg);
            msgs.push(msg);
        }

        let acc = &tpu.accounting_stage.acc;
        let alice_balance = acc.get_balance(&alice.keypair().pubkey()).unwrap();
        assert_eq!(alice_balance, alice_ref_balance);

        let bob_balance = acc.get_balance(&bob_keypair.pubkey()).unwrap();
        assert_eq!(bob_balance, starting_balance - alice_ref_balance);

        exit.store(true, Ordering::Relaxed);
        for t in threads {
            t.join().expect("join");
        }
        t2_gossip.join().expect("join");
        t2_listen.join().expect("join");
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_l_gossip.join().expect("join");
        t_l_listen.join().expect("join");
    }

    #[test]
    fn test_entry_to_blobs() {
        let zero = Hash::default();
        let keypair = KeyPair::new();
        let tr0 = Event::Transaction(Transaction::new(&keypair, keypair.pubkey(), 0, zero));
        let tr1 = Event::Transaction(Transaction::new(&keypair, keypair.pubkey(), 1, zero));
        let e0 = entry::create_entry(&zero, 0, vec![tr0.clone(), tr1.clone()]);

        let entry_list = vec![e0; 1000];
        let blob_recycler = BlobRecycler::default();
        let mut blob_q = VecDeque::new();
        Tpu::process_entry_list_into_blobs(&entry_list, &blob_recycler, &mut blob_q);
        let serialized_entry_list = serialize(&entry_list).unwrap();
        let mut num_blobs_ref = serialized_entry_list.len() / BLOB_SIZE;
        if serialized_entry_list.len() % BLOB_SIZE != 0 {
            num_blobs_ref += 1
        }
        trace!("len: {} ref_len: {}", blob_q.len(), num_blobs_ref);
        assert!(blob_q.len() > num_blobs_ref);
    }
}
