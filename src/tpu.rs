//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.
//!
//! ```text
//!             .----------------------------------------------------.
//!             |  TPU                      .-------------.          |
//!             |                           | PoH Service |          |
//!             |                           `-------+-----`          |
//!             |                              ^    |                |
//!             |                              |    v                |
//!             |  .-------.  .-----------.  .-+-------.   .-------. |
//! .---------. |  | Fetch |  | SigVerify |  | Banking |   | Write | |  .------------.
//! | Clients |--->| Stage |->|   Stage   |->|  Stage  |-->| Stage +--->| Validators |
//! `---------` |  |       |  |           |  |         |   |       | |  `------------`
//!             |  `-------`  `-----------`  `----+----`   `---+---` |
//!             |                                 |            |     |
//!             |                                 |            |     |
//!             |                                 |            |     |
//!             |                                 |            |     |
//!             `---------------------------------|------------|-----`
//!                                               |            |
//!                                               v            v
//!                                            .------.    .--------.
//!                                            | Bank |    | Ledger |
//!                                            `------`    `--------`
//! ```

use bank::Bank;
use banking_stage::{BankingStage, Config};
use crdt::Crdt;
use entry::Entry;
use fetch_stage::FetchStage;
use service::Service;
use signature::Keypair;
use sigverify_stage::SigVerifyStage;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::thread;
use write_stage::{WriteStage, WriteStageReturnType};

pub enum TpuReturnType {
    LeaderRotation,
}

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    write_stage: WriteStage,
    exit: Arc<AtomicBool>,
}

impl Tpu {
    pub fn new(
        keypair: Arc<Keypair>,
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        tick_duration: Config,
        transactions_sockets: Vec<UdpSocket>,
        ledger_path: &str,
        sigverify_disabled: bool,
        entry_height: u64,
    ) -> (Self, Receiver<Vec<Entry>>, Arc<AtomicBool>) {
        let exit = Arc::new(AtomicBool::new(false));

        let (fetch_stage, packet_receiver) = FetchStage::new(transactions_sockets, exit.clone());

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, entry_receiver) =
            BankingStage::new(&bank, verified_receiver, tick_duration);

        let (write_stage, entry_forwarder) = WriteStage::new(
            keypair,
            bank.clone(),
            crdt.clone(),
            ledger_path,
            entry_receiver,
            entry_height,
        );

        let tpu = Tpu {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            write_stage,
            exit: exit.clone(),
        };
        (tpu, entry_forwarder, exit)
    }

    pub fn exit(&self) -> () {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<Option<TpuReturnType>> {
        self.fetch_stage.close();
        self.join()
    }
}

impl Service for Tpu {
    type JoinReturnType = Option<TpuReturnType>;

    fn join(self) -> thread::Result<(Option<TpuReturnType>)> {
        self.fetch_stage.join()?;
        self.sigverify_stage.join()?;
        self.banking_stage.join()?;
        match self.write_stage.join()? {
            WriteStageReturnType::LeaderRotation => Ok(Some(TpuReturnType::LeaderRotation)),
            _ => Ok(None),
        }
    }
}
