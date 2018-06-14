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
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use streamer::BlobReceiver;
use write_stage::WriteStage;

pub struct Tpu {
    pub blob_receiver: BlobReceiver,
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
    ) -> Self {
        let packet_recycler = PacketRecycler::default();

        let fetch_stage =
            FetchStage::new(transactions_socket, exit.clone(), packet_recycler.clone());

        let sigverify_stage = SigVerifyStage::new(exit.clone(), fetch_stage.packet_receiver);

        let banking_stage = BankingStage::new(
            bank.clone(),
            exit.clone(),
            sigverify_stage.verified_receiver,
            packet_recycler.clone(),
        );

        let record_stage = match tick_duration {
            Some(tick_duration) => RecordStage::new_with_clock(
                banking_stage.signal_receiver,
                &bank.last_id(),
                tick_duration,
            ),
            None => RecordStage::new(banking_stage.signal_receiver, &bank.last_id()),
        };

        let write_stage = WriteStage::new(
            bank.clone(),
            exit.clone(),
            blob_recycler.clone(),
            Mutex::new(writer),
            record_stage.entry_receiver,
        );
        let mut thread_hdls = vec![
            banking_stage.thread_hdl,
            record_stage.thread_hdl,
            write_stage.thread_hdl,
        ];
        thread_hdls.extend(fetch_stage.thread_hdls.into_iter());
        thread_hdls.extend(sigverify_stage.thread_hdls.into_iter());
        Tpu {
            blob_receiver: write_stage.blob_receiver,
            thread_hdls,
        }
    }
}
