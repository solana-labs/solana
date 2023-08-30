use {
    crossbeam_channel::{Receiver, RecvTimeoutError},
    std::{
        result::Result,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct StatsReporterService {
    thread_hdl: JoinHandle<()>,
}

impl StatsReporterService {
    pub fn new(
        reporting_receiver: Receiver<Box<dyn FnOnce() + Send>>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solStatsReport".to_owned())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                if let Err(e) = Self::receive_reporting_func(&reporting_receiver) {
                    match e {
                        RecvTimeoutError::Disconnected => break,
                        RecvTimeoutError::Timeout => (),
                    }
                }
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()?;
        Ok(())
    }

    fn receive_reporting_func(
        r: &Receiver<Box<dyn FnOnce() + Send>>,
    ) -> Result<(), RecvTimeoutError> {
        let timer = Duration::new(1, 0);
        let func = r.recv_timeout(timer)?;
        func();
        Ok(())
    }
}
