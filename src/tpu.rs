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
use banking_stage::{BankingStage, BankingStageReturnType};
use entry::Entry;
use fetch_stage::FetchStage;
use hash::Hash;
use ledger_write_stage::LedgerWriteStage;
use poh_service::Config;
use service::Service;
use sigverify_stage::SigVerifyStage;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;

pub enum TpuReturnType {
    LeaderRotation,
}

pub struct Tpu {
    fetch_stage: FetchStage,
    sigverify_stage: SigVerifyStage,
    banking_stage: BankingStage,
    ledger_write_stage: LedgerWriteStage,
    exit: Arc<AtomicBool>,
}

impl Tpu {
    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    pub fn new(
        bank: &Arc<Bank>,
        tick_duration: Config,
        transactions_sockets: Vec<UdpSocket>,
        ledger_path: &str,
        sigverify_disabled: bool,
        max_tick_height: Option<u64>,
        last_entry_id: &Hash,
    ) -> (Self, Receiver<Vec<Entry>>, Arc<AtomicBool>) {
        let exit = Arc::new(AtomicBool::new(false));

        let (fetch_stage, packet_receiver) = FetchStage::new(transactions_sockets, exit.clone());

        let (sigverify_stage, verified_receiver) =
            SigVerifyStage::new(packet_receiver, sigverify_disabled);

        let (banking_stage, entry_receiver) = BankingStage::new(
            &bank,
            verified_receiver,
            tick_duration,
            last_entry_id,
            max_tick_height,
        );

        let (ledger_entry_sender, entry_forwarder) = channel();
        let ledger_write_stage =
            LedgerWriteStage::new(Some(ledger_path), entry_receiver, Some(ledger_entry_sender));

        let tpu = Tpu {
            fetch_stage,
            sigverify_stage,
            banking_stage,
            ledger_write_stage,
            exit: exit.clone(),
        };

        (tpu, entry_forwarder, exit)
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn is_exited(&self) -> bool {
        self.exit.load(Ordering::Relaxed)
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
        self.ledger_write_stage.join()?;
        match self.banking_stage.join()? {
            Some(BankingStageReturnType::LeaderRotation) => Ok(Some(TpuReturnType::LeaderRotation)),
            _ => Ok(None),
        }
    }
}
