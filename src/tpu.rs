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
use crdt::Crdt;
use entry::Entry;
use fetch_stage::FetchStage;
use packet::{BlobRecycler, PacketRecycler};
use record_stage::RecordStage;
use service::Service;
use signature::Keypair;
use sigverify_stage::SigVerifyStage;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use write_stage::WriteStage;

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    record_stage: RecordStage,
    write_stage: WriteStage,
}

impl Tpu {
    pub fn new(
        keypair: Keypair,
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        tick_duration: Option<Duration>,
        transactions_sockets: Vec<UdpSocket>,
        blob_recycler: &BlobRecycler,
        exit: Arc<AtomicBool>,
        ledger_path: &str,
        sigverify_disabled: bool,
        entry_height: u64,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let mut packet_recycler = PacketRecycler::default();
        packet_recycler.set_name("tpu::Packet");

        let (fetch_stage, packet_receiver) =
            FetchStage::new(transactions_sockets, exit, &packet_recycler);

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, signal_receiver) =
            BankingStage::new(bank.clone(), verified_receiver, packet_recycler.clone());

        let (record_stage, entry_receiver) = match tick_duration {
            Some(tick_duration) => {
                RecordStage::new_with_clock(signal_receiver, bank.clone(), tick_duration)
            }
            None => RecordStage::new(signal_receiver, bank.clone()),
        };

        let (write_stage, entry_forwarder) = WriteStage::new(
            keypair,
            bank.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            ledger_path,
            entry_receiver,
            entry_height,
        );

        let tpu = Tpu {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            record_stage,
            write_stage,
        };
        (tpu, entry_forwarder)
    }

    pub fn close(self) -> thread::Result<()> {
        self.fetch_stage.close();
        self.join()
    }
}

impl Service for Tpu {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.fetch_stage.join()?;
        self.sigverify_stage.join()?;
        self.banking_stage.join()?;
        self.record_stage.join()?;
        self.write_stage.join()?;

        Ok(())
    }
}
