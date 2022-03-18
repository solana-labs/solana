//! PohTimingReportService module
use {
    crate::poh_timing_reporter::PohTimingReporter,
    crossbeam_channel::Receiver,
    solana_metrics::datapoint::PohTimingPoint,
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

/// Timeout to wait on the poh timing points from the channel
const POH_TIMING_RECEIVER_TIMEOUT_MILLISECONDS: u64 = 1000;

pub type PohTimingReceiver = Receiver<(Slot, PohTimingPoint)>;

/// The `poh_timing_report_service` receives signals of relevant timing points
/// during the processing of a slot, (i.e. from blockstore and poh), aggregate and
/// report the result as datapoints.
pub struct PohTimingReportService {
    t_poh_timing: JoinHandle<()>,
}

impl PohTimingReportService {
    pub fn new(receiver: PohTimingReceiver, exit: Arc<AtomicBool>) -> Self {
        let exit_signal = exit;
        let mut poh_timing_reporter = PohTimingReporter::new();
        let t_poh_timing = Builder::new()
            .name("poh_timing_report".to_string())
            .spawn(move || loop {
                if exit_signal.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok((slot, timing_point)) = receiver.recv_timeout(Duration::from_millis(
                    POH_TIMING_RECEIVER_TIMEOUT_MILLISECONDS,
                )) {
                    poh_timing_reporter.process(slot, timing_point);
                }
            })
            .unwrap();
        Self { t_poh_timing }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_poh_timing.join()
    }
}

#[cfg(test)]
mod test {
    use {super::*, crossbeam_channel::unbounded};

    #[test]
    /// Test the life cycle of the PohTimingReportService
    fn test_poh_timing_report_service() {
        let (poh_timing_point_sender, poh_timing_point_receiver) = unbounded();
        let exit = Arc::new(AtomicBool::new(false));
        // Create the service
        let poh_timing_report_service =
            PohTimingReportService::new(poh_timing_point_receiver, exit.clone());

        // Send PohTimingPoints
        let _ = poh_timing_point_sender.send((42, PohTimingPoint::PohSlotStart(100)));
        let _ = poh_timing_point_sender.send((42, PohTimingPoint::PohSlotEnd(200)));
        let _ = poh_timing_point_sender.send((42, PohTimingPoint::FullSlotReceived(150)));

        // Shutdown the service
        exit.store(true, Ordering::Relaxed);
        poh_timing_report_service
            .join()
            .expect("poh_timing_report_service completed");
    }
}
