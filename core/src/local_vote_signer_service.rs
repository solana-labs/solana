//! The `local_vote_signer_service` can be started locally to sign validator votes

use solana_net_utils::PortRange;
use solana_vote_signer::rpc::VoteSignerRpcService;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};

pub struct LocalVoteSignerService {
    thread: JoinHandle<()>,
    exit: Arc<AtomicBool>,
}

impl LocalVoteSignerService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(port_range: PortRange) -> (Self, SocketAddr) {
        let addr = solana_net_utils::find_available_port_in_range(port_range)
            .map(|port| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))
            .expect("Failed to find an available port for local vote signer service");
        let exit = Arc::new(AtomicBool::new(false));
        let thread_exit = exit.clone();
        let thread = Builder::new()
            .name("solana-vote-signer".to_string())
            .spawn(move || {
                let service = VoteSignerRpcService::new(addr, &thread_exit);
                service.join().unwrap();
            })
            .unwrap();

        (Self { thread, exit }, addr)
    }

    pub fn join(self) -> thread::Result<()> {
        self.exit.store(true, Ordering::Relaxed);
        self.thread.join()
    }
}
