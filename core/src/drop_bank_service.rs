use {
    crossbeam_channel::Receiver,
    solana_measure::measure::Measure,
    solana_runtime::bank::Bank,
    std::{
        sync::Arc,
        thread::{self, Builder, JoinHandle},
    },
};

pub struct DropBankService {
    thread_hdl: JoinHandle<()>,
}

impl DropBankService {
    pub fn new(bank_receiver: Receiver<Vec<Arc<Bank>>>) -> Self {
        let thread_hdl = Builder::new()
            .name("solDropBankSrvc".to_string())
            .spawn(move || {
                for banks in bank_receiver.iter() {
                    let len = banks.len();
                    let mut dropped_banks_time = Measure::start("drop_banks");
                    drop(banks);
                    dropped_banks_time.stop();
                    if dropped_banks_time.as_ms() > 10 {
                        datapoint_info!(
                            "handle_new_root-dropped_banks",
                            ("elapsed_ms", dropped_banks_time.as_ms(), i64),
                            ("len", len, i64)
                        );
                    }
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
