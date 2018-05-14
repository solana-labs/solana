//! The `request_stage` processes thin client Request messages.

use accountant::Accountant;
use bincode::{deserialize, serialize};
use event::Event;
use packet;
use packet::SharedPackets;
use rayon::prelude::*;
use recorder::Signal;
use request::{Request, Response};
use result::Result;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use std::time::Instant;
use streamer;
use timing;

pub struct RequestProcessor {
    accountant: Arc<Accountant>,
}

impl RequestProcessor {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(accountant: Arc<Accountant>) -> Self {
        RequestProcessor { accountant }
    }

    /// Process Request items sent by clients.
    fn process_request(
        &self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetBalance { key } => {
                let val = self.accountant.get_balance(&key);
                let rsp = (Response::Balance { key, val }, rsp_addr);
                info!("Response::Balance {:?}", rsp);
                Some(rsp)
            }
            Request::GetLastId => {
                let id = self.accountant.last_id();
                let rsp = (Response::LastId { id }, rsp_addr);
                info!("Response::LastId {:?}", rsp);
                Some(rsp)
            }
            Request::GetTransactionCount => {
                let transaction_count = self.accountant.transaction_count() as u64;
                let rsp = (Response::TransactionCount { transaction_count }, rsp_addr);
                info!("Response::TransactionCount {:?}", rsp);
                Some(rsp)
            }
            Request::Transaction(_) => unreachable!(),
        }
    }

    pub fn process_requests(
        &self,
        reqs: Vec<(Request, SocketAddr)>,
    ) -> Vec<(Response, SocketAddr)> {
        reqs.into_iter()
            .filter_map(|(req, rsp_addr)| self.process_request(req, rsp_addr))
            .collect()
    }

    fn deserialize_requests(p: &packet::Packets) -> Vec<Option<(Request, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            })
            .collect()
    }

    // Copy-paste of deserialize_requests() because I can't figure out how to
    // route the lifetimes in a generic version.
    pub fn deserialize_events(p: &packet::Packets) -> Vec<Option<(Event, SocketAddr)>> {
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

    pub fn process_request_packets(
        &self,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        signal_sender: &Sender<Signal>,
        blob_sender: &streamer::BlobSender,
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
            let reqs = Self::deserialize_requests(&msgs.read().unwrap());
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
            let results = self.accountant.process_verified_events(events);
            let events = results.into_iter().filter_map(|x| x.ok()).collect();
            signal_sender.send(Signal::Events(events))?;
            debug!("done process_events");

            debug!("process_requests");
            let rsps = self.process_requests(reqs);
            debug!("done process_requests");

            let blobs = Self::serialize_responses(rsps, blob_recycler)?;
            if !blobs.is_empty() {
                info!("process: sending blobs: {}", blobs.len());
                //don't wake up the other side if there is nothing
                blob_sender.send(blobs)?;
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
}
