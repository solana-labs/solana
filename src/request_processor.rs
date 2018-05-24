//! The `request_stage` processes thin client Request messages.

use bank::Bank;
use bincode::{deserialize, serialize};
use packet;
use packet::SharedPackets;
use rayon::prelude::*;
use request::{Request, Response};
use result::Result;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use streamer;
use timing;
use transaction::Transaction;

pub struct RequestProcessor {
    bank: Arc<Bank>,
}

impl RequestProcessor {
    /// Create a new Tpu that wraps the given Bank.
    pub fn new(bank: Arc<Bank>) -> Self {
        RequestProcessor { bank }
    }

    /// Process Request items sent by clients.
    fn process_request(
        &self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetBalance { key } => {
                let val = self.bank.get_balance(&key);
                let rsp = (Response::Balance { key, val }, rsp_addr);
                info!("Response::Balance {:?}", rsp);
                Some(rsp)
            }
            Request::GetLastId => {
                let id = self.bank.last_id();
                let rsp = (Response::LastId { id }, rsp_addr);
                info!("Response::LastId {:?}", rsp);
                Some(rsp)
            }
            Request::GetTransactionCount => {
                let transaction_count = self.bank.transaction_count() as u64;
                let rsp = (Response::TransactionCount { transaction_count }, rsp_addr);
                info!("Response::TransactionCount {:?}", rsp);
                Some(rsp)
            }
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
    pub fn deserialize_events(p: &packet::Packets) -> Vec<Option<(Transaction, SocketAddr)>> {
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
        packet_receiver: &Receiver<SharedPackets>,
        blob_sender: &streamer::BlobSender,
        packet_recycler: &packet::PacketRecycler,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let (batch, batch_len) = streamer::recv_batch(packet_receiver)?;

        info!(
            "@{:?} request_stage: processing: {}",
            timing::timestamp(),
            batch_len
        );

        let mut reqs_len = 0;
        let proc_start = Instant::now();
        for msgs in batch {
            let reqs: Vec<_> = Self::deserialize_requests(&msgs.read().unwrap())
                .into_iter()
                .filter_map(|x| x)
                .collect();
            reqs_len += reqs.len();

            let rsps = self.process_requests(reqs);

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
            batch_len,
            total_time_ms,
            reqs_len,
            (reqs_len as f32) / (total_time_s)
        );
        Ok(())
    }
}
