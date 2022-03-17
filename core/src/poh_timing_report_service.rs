//! The `poh_timing_report_service` reports slot poh start/end time against slot shreds receiving
//! time from the block producer.

use {
    crate::poh_timing_reporter::PohTimingReporter,
    crossbeam_channel::Receiver,
    solana_sdk::clock::Slot,
    std::{
        string::ToString,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

/// Timeout to wait on the poh timestamp from the channel
const POH_TIMING_RECEIVER_TIMEOUT_MILLISECONDS: u64 = 1000;

pub type PohTimingReceiver = Receiver<(Slot, String, u64)>;
pub struct PohTimingReportService {
    t_poh_timing: JoinHandle<()>,
}

impl PohTimingReportService {
    pub fn new(receiver: PohTimingReceiver, exit: Arc<AtomicBool>) -> Self {
        //let (sender, receiver) = unbounded();

        let exit_signal = exit.clone();
        let mut poh_timing_reporter = PohTimingReporter::new();
        let t_poh_timing = Builder::new()
            .name("poh_timing_report".to_string())
            .spawn(move || loop {
                if exit_signal.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok((slot, name, ts)) = receiver.recv_timeout(Duration::from_millis(
                    POH_TIMING_RECEIVER_TIMEOUT_MILLISECONDS,
                )) {
                    poh_timing_reporter.process(slot, &name, ts);
                }
            })
            .unwrap();
        Self { t_poh_timing }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_poh_timing.join()
    }
}
