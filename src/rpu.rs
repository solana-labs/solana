//! The `rpu` module implements the Request Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use bank::Bank;
use packet;
use request_processor::RequestProcessor;
use request_stage::RequestStage;
use sig_verify_stage::SigVerifyStage;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use streamer;

pub struct Rpu {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Rpu {
    pub fn new(
        bank: Arc<Bank>,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let packet_recycler = packet::PacketRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver = streamer::receiver(
            requests_socket,
            exit.clone(),
            packet_recycler.clone(),
            packet_sender,
        );

        let sig_verify_stage = SigVerifyStage::new(exit.clone(), packet_receiver);

        let blob_recycler = packet::BlobRecycler::default();
        let request_processor = RequestProcessor::new(bank.clone());
        let request_stage = RequestStage::new(
            request_processor,
            exit.clone(),
            sig_verify_stage.verified_receiver,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            request_stage.blob_receiver,
        );

        let mut thread_hdls = vec![t_receiver, t_responder, request_stage.thread_hdl];
        thread_hdls.extend(sig_verify_stage.thread_hdls.into_iter());

        Rpu { thread_hdls }
    }
}
