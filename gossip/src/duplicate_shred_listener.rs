use {
    crate::{
        cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS},
        crds::Cursor,
        duplicate_shred::DuplicateShred,
    },
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
};

pub trait DuplicateShredHandlerTrait: Send {
    fn handle(&mut self, data: DuplicateShred);
}

pub struct DuplicateShredListener {
    thread_hdl: JoinHandle<()>,
}

// Right now we only need to process duplicate proof, in the future the receiver
// should be a map from enum value to handlers.
impl DuplicateShredListener {
    pub fn new(
        exit: Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        handler: impl DuplicateShredHandlerTrait + 'static,
    ) -> Self {
        let listen_thread = Builder::new()
            .name("solCiEntryLstnr".to_string())
            .spawn(move || {
                Self::recv_loop(exit, &cluster_info, handler);
            })
            .unwrap();

        Self {
            thread_hdl: listen_thread,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }

    // Here we are sending data one by one rather than in a batch because in the future
    // we may send different type of CrdsData to different senders.
    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &ClusterInfo,
        mut handler: impl DuplicateShredHandlerTrait + 'static,
    ) {
        let mut cursor = Cursor::default();
        while !exit.load(Ordering::Relaxed) {
            let entries: Vec<DuplicateShred> = cluster_info.get_duplicate_shreds(&mut cursor);
            for x in entries {
                handler.handle(x);
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            cluster_info::Node, duplicate_shred::tests::new_rand_shred,
            duplicate_shred_listener::DuplicateShredHandlerTrait,
        },
        solana_ledger::shred::Shredder,
        solana_sdk::signature::{Keypair, Signer},
        solana_streamer::socket::SocketAddrSpace,
        std::sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        },
    };
    struct FakeHandler {
        count: Arc<AtomicU32>,
    }

    impl FakeHandler {
        fn new(count: Arc<AtomicU32>) -> Self {
            Self { count }
        }
    }

    impl DuplicateShredHandlerTrait for FakeHandler {
        fn handle(&mut self, data: DuplicateShred) {
            assert!(data.num_chunks() > 0);
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_listener_get_entries() {
        let host1_key = Arc::new(Keypair::new());
        let node = Node::new_localhost_with_pubkey(&host1_key.pubkey());
        let cluster_info = Arc::new(ClusterInfo::new(
            node.info,
            host1_key,
            SocketAddrSpace::Unspecified,
        ));
        let exit = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicU32::new(0));
        let handler = FakeHandler::new(count.clone());
        let listener = DuplicateShredListener::new(exit.clone(), cluster_info.clone(), handler);
        let mut rng = rand::thread_rng();
        let (slot, parent_slot, reference_tick, version) = (53084024, 53084023, 0, 0);
        let shredder = Shredder::new(slot, parent_slot, reference_tick, version).unwrap();
        let next_shred_index = 353;
        let leader = Arc::new(Keypair::new());
        let shred1 = new_rand_shred(&mut rng, next_shred_index, &shredder, &leader);
        let shred2 = new_rand_shred(&mut rng, next_shred_index, &shredder, &leader);
        assert!(cluster_info
            .push_duplicate_shred(&shred1, shred2.payload())
            .is_ok());
        cluster_info.flush_push_queue();
        sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        assert_eq!(count.load(Ordering::Relaxed), 3);
        exit.store(true, Ordering::Relaxed);
        assert!(listener.join().is_ok());
    }
}
