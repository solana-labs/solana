use std::thread::{JoinHandle, Result};

pub trait Service {
    fn thread_hdls(self) -> Vec<JoinHandle<()>>;
    fn join(self) -> Result<()>;
}
