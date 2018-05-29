//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use bank::Bank;
use banking_stage::BankingStage;
use hash::Hash;
use packet;
use record_stage::RecordStage;
use sigverify_stage::SigVerifyStage;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use streamer;
use write_stage::WriteStage;

pub struct Tpu {
    pub blob_receiver: streamer::BlobReceiver,
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Tpu {
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        start_hash: Hash,
        tick_duration: Option<Duration>,
        transactions_socket: UdpSocket,
        blob_recycler: packet::BlobRecycler,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let packet_recycler = packet::PacketRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver = streamer::receiver(
            transactions_socket,
            exit.clone(),
            packet_recycler.clone(),
            packet_sender,
        );

        let sigverify_stage = SigVerifyStage::new(exit.clone(), packet_receiver);

        let banking_stage = BankingStage::new(
            bank.clone(),
            exit.clone(),
            sigverify_stage.verified_receiver,
            packet_recycler.clone(),
        );

        let record_stage =
            RecordStage::new(banking_stage.signal_receiver, &start_hash, tick_duration);

        let write_stage = WriteStage::new(
            bank.clone(),
            exit.clone(),
            blob_recycler.clone(),
            Mutex::new(writer),
            record_stage.entry_receiver,
        );

        let blob_receiver = write_stage.blob_receiver;
        let mut thread_hdls = vec![
            t_receiver,
            banking_stage.thread_hdl,
            record_stage.thread_hdl,
            write_stage.thread_hdl,
        ];
        thread_hdls.extend(sigverify_stage.thread_hdls.into_iter());
        Tpu {
            blob_receiver,
            thread_hdls,
        }
    }
}
