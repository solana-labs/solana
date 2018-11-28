//! The `tpu_forwarder` module implements a validator's
//!  transaction processing unit responsibility, which
//!  forwards received packets to the current leader

use cluster_info::ClusterInfo;
use contact_info::ContactInfo;
use counter::Counter;
use log::Level;
use result::Result;
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use streamer::{self, PacketReceiver};

pub struct TpuForwarder {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl TpuForwarder {
    fn forward(receiver: &PacketReceiver, cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        let my_id = cluster_info
            .read()
            .expect("cluster_info.read() in TpuForwarder::forward()")
            .id();

        loop {
            let msgs = receiver.recv()?;

            inc_new_counter_info!(
                "tpu_forwarder-msgs_received",
                msgs.read().unwrap().packets.len()
            );

            if let Some(leader_data) = cluster_info
                .read()
                .expect("cluster_info.read() in TpuForwarder::forward()")
                .leader_data()
                .cloned()
            {
                if leader_data.id == my_id || !ContactInfo::is_valid_address(&leader_data.tpu) {
                    // weird cases, but we don't want to broadcast, send to ANY, or
                    // induce an infinite loop, but this shouldn't happen, or shouldn't be true for long...
                    continue;
                }

                for m in msgs.write().unwrap().packets.iter_mut() {
                    m.meta.set_addr(&leader_data.tpu);
                }
                msgs.read().unwrap().send_to(&socket)?
            }
        }
    }

    pub fn new(sockets: Vec<UdpSocket>, cluster_info: Arc<RwLock<ClusterInfo>>) -> Self {
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();

        let mut thread_hdls: Vec<_> = sockets
            .into_iter()
            .map(|socket| {
                streamer::receiver(
                    Arc::new(socket),
                    exit.clone(),
                    sender.clone(),
                    "tpu-forwarder",
                )
            }).collect();

        let thread_hdl = Builder::new()
            .name("solana-tpu_forwarder".to_string())
            .spawn(move || {
                let _ignored = Self::forward(&receiver, &cluster_info);
                ()
            }).unwrap();

        thread_hdls.push(thread_hdl);

        TpuForwarder { exit, thread_hdls }
    }

    pub fn close(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

impl Service for TpuForwarder {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.close();
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_info::ClusterInfo;
    use contact_info::ContactInfo;
    use netutil::bind_in_range;
    use solana_sdk::pubkey::Pubkey;
    use std::net::UdpSocket;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    #[ignore]
    pub fn test_tpu_forwarder() {
        let nodes: Vec<_> = (0..3)
            .map(|_| {
                let (port, s) = bind_in_range((8000, 10000)).unwrap();
                s.set_nonblocking(true).unwrap();
                (
                    s,
                    ContactInfo::new_with_socketaddr(&socketaddr!([127, 0, 0, 1], port)),
                )
            }).collect();

        let mut cluster_info = ClusterInfo::new(nodes[0].1.clone());

        cluster_info.insert_info(nodes[1].1.clone());
        cluster_info.insert_info(nodes[2].1.clone());
        cluster_info.insert_info(Default::default());

        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let (transaction_port, transaction_socket) = bind_in_range((8000, 10000)).unwrap();
        let transaction_addr = socketaddr!([127, 0, 0, 1], transaction_port);

        let tpu_forwarder = TpuForwarder::new(vec![transaction_socket], cluster_info.clone());

        let test_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        // no leader set in cluster_info, drop the "transaction"
        test_socket
            .send_to(b"alice pays bob", &transaction_addr)
            .unwrap();
        sleep(Duration::from_millis(100));

        let mut data = vec![0u8; 64];
        // should be nothing on any socket
        assert!(nodes[0].0.recv_from(&mut data).is_err());
        assert!(nodes[1].0.recv_from(&mut data).is_err());
        assert!(nodes[2].0.recv_from(&mut data).is_err());

        // set leader to host with no tpu
        cluster_info.write().unwrap().set_leader(Pubkey::default());
        test_socket
            .send_to(b"alice pays bart", &transaction_addr)
            .unwrap();
        sleep(Duration::from_millis(100));

        let mut data = vec![0u8; 64];
        // should be nothing on any socket ncp
        assert!(nodes[0].0.recv_from(&mut data).is_err());
        assert!(nodes[1].0.recv_from(&mut data).is_err());
        assert!(nodes[2].0.recv_from(&mut data).is_err());

        cluster_info.write().unwrap().set_leader(nodes[0].1.id); // set leader to myself, bytes get dropped :-(

        test_socket
            .send_to(b"alice pays bill", &transaction_addr)
            .unwrap();
        sleep(Duration::from_millis(100));

        // should *still* be nothing on any socket
        assert!(nodes[0].0.recv_from(&mut data).is_err());
        assert!(nodes[1].0.recv_from(&mut data).is_err());
        assert!(nodes[2].0.recv_from(&mut data).is_err());

        cluster_info.write().unwrap().set_leader(nodes[1].1.id); // set leader to node[1]

        test_socket
            .send_to(b"alice pays chuck", &transaction_addr)
            .unwrap();
        sleep(Duration::from_millis(100));

        // should only be data on node[1]'s socket
        assert!(nodes[0].0.recv_from(&mut data).is_err());
        assert!(nodes[2].0.recv_from(&mut data).is_err());

        assert!(nodes[1].0.recv_from(&mut data).is_ok());
        assert_eq!(&data[..b"alice pays chuck".len()], b"alice pays chuck");

        assert!(tpu_forwarder.join().is_ok());
    }

}
