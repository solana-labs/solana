use solana_runtime::{
    bank_forks::{BankForks},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
        RwLock
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub struct SamplePerformanceService {
    thread_hdl: JoinHandle<()>,
}

impl SamplePerformanceService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        bank_forks: &Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let bank_forks = bank_forks.clone();
        info!("Starting SamplePerformance service");
        let thread_hdl = Builder::new()
            .name("sample-performance".to_string())
            .spawn(move || {
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
        
                    let forks = bank_forks.read().unwrap();

                    let bank = forks.working_bank();
       
                    print!("Sample performance\n");
        
                    println!("{:?}", forks.highest_slot());
        
                    println!("{:?}", bank.transaction_count());
        
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
