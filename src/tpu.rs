//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.
//!
//! ```text
//!             .---------------------------------------------------------------.
//!             |  TPU                                     .-----.              |
//!             |                                          | PoH |              |
//!             |                                          `--+--`              |
//!             |                                             |                 |
//!             |                                             v                 |
//!             |  .-------.  .-----------.  .---------.  .--------.  .-------. |
//! .---------. |  | Fetch |  | SigVerify |  | Banking |  | Record |  | Write | |  .------------.
//! | Clients |--->| Stage |->|   Stage   |->|  Stage  |->| Stage  |->| Stage +--->| Validators |
//! `---------` |  |       |  |           |  |         |  |        |  |       | |  `------------`
//!             |  `-------`  `-----------`  `----+----`  `--------`  `---+---` |
//!             |                                 |                       |     |
//!             |                                 |                       |     |
//!             |                                 |                       |     |
//!             |                                 |                       |     |
//!             `---------------------------------|-----------------------|-----`
//!                                               |                       |
//!                                               v                       v
//!                                            .------.               .--------.
//!                                            | Bank |               | Ledger |
//!                                            `------`               `--------`
//! ```

use bank::Bank;
use banking_stage::BankingStage;
use fetch_stage::FetchStage;
use packet::{BlobRecycler, PacketRecycler};
use record_stage::RecordStage;
use sigverify_stage::SigVerifyStage;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use streamer::BlobReceiver;
use write_stage::WriteStage;

pub struct Tpu {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Tpu {
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        tick_duration: Option<Duration>,
        transactions_socket: UdpSocket,
        blob_recycler: BlobRecycler,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> (Self, BlobReceiver) {
        let packet_recycler = PacketRecycler::default();

        let (fetch_stage, packet_receiver) =
            FetchStage::new(transactions_socket, exit.clone(), packet_recycler.clone());

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(exit.clone(), packet_receiver);

        let (banking_stage, signal_receiver) = BankingStage::new(
            bank.clone(),
            exit.clone(),
            verified_receiver,
            packet_recycler.clone(),
        );

        let (record_stage, entry_receiver) = match tick_duration {
            Some(tick_duration) => {
                RecordStage::new_with_clock(signal_receiver, &bank.last_id(), tick_duration)
            }
            None => RecordStage::new(signal_receiver, &bank.last_id()),
        };

        let (write_stage, blob_receiver) = WriteStage::new(
            bank.clone(),
            exit.clone(),
            blob_recycler.clone(),
            writer,
            entry_receiver,
        );
        let mut thread_hdls = vec![
            banking_stage.thread_hdl,
            record_stage.thread_hdl,
            write_stage.thread_hdl,
        ];
        thread_hdls.extend(fetch_stage.thread_hdls.into_iter());
        thread_hdls.extend(sigverify_stage.thread_hdls.into_iter());
        (Tpu { thread_hdls }, blob_receiver)
    }
}
