//! The `ncp` module implements the network control plane.

use crdt::Crdt;
use packet::{BlobRecycler, SharedBlob};
use result::Result;
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use streamer;

pub struct Ncp {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl Ncp {
    pub fn new(
        crdt: Arc<RwLock<Crdt>>,
        window: Arc<RwLock<Vec<Option<SharedBlob>>>>,
        gossip_listen_socket: UdpSocket,
        gossip_send_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Result<Ncp> {
        let blob_recycler = BlobRecycler::default();
        let (request_sender, request_receiver) = channel();
        trace!(
            "Ncp: id: {:?}, listening on: {:?}",
            &crdt.read().unwrap().me[..4],
            gossip_listen_socket.local_addr().unwrap()
        );
        let t_receiver = streamer::blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            gossip_listen_socket,
            request_sender,
        )?;
        let (response_sender, response_receiver) = channel();
        let t_responder =
            streamer::responder(gossip_send_socket, blob_recycler.clone(), response_receiver);
        let t_listen = Crdt::listen(
            crdt.clone(),
            window,
            blob_recycler.clone(),
            request_receiver,
            response_sender.clone(),
            exit.clone(),
        );
        let t_gossip = Crdt::gossip(crdt.clone(), blob_recycler, response_sender, exit);
        let thread_hdls = vec![t_receiver, t_responder, t_listen, t_gossip];
        Ok(Ncp { thread_hdls })
    }
}

impl Service for Ncp {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        self.thread_hdls
    }

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls() {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crdt::{Crdt, TestNode};
    use ncp::Ncp;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};

    #[test]
    #[ignore]
    // test that stage will exit when flag is set
    fn test_exit() {
        let exit = Arc::new(AtomicBool::new(false));
        let tn = TestNode::new();
        let crdt = Crdt::new(tn.data.clone());
        let c = Arc::new(RwLock::new(crdt));
        let w = Arc::new(RwLock::new(vec![]));
        let d = Ncp::new(
            c.clone(),
            w,
            tn.sockets.gossip,
            tn.sockets.gossip_send,
            exit.clone(),
        ).unwrap();
        exit.store(true, Ordering::Relaxed);
        for t in d.thread_hdls {
            t.join().expect("thread join");
        }
    }
}
