//! The `tpu_forwarder` module implements a validator's
//!  transaction processing unit responsibility, which
//!  forwards received packets to the current leader

use crate::cluster_info::ClusterInfo;
use crate::contact_info::ContactInfo;
use crate::counter::Counter;
use crate::result::Result;
use crate::service::Service;
use crate::streamer::{self, PacketReceiver};
use log::Level;
use solana_sdk::pubkey::Pubkey;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};

fn get_forwarding_addr(leader_data: Option<&ContactInfo>, my_id: &Pubkey) -> Option<SocketAddr> {
    let leader_data = leader_data?;
    if leader_data.id == *my_id || !ContactInfo::is_valid_address(&leader_data.tpu) {
        // weird cases, but we don't want to broadcast, send to ANY, or
        // induce an infinite loop, but this shouldn't happen, or shouldn't be true for long...
        return None;
    }
    Some(leader_data.tpu)
}

pub struct TpuForwarder {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl TpuForwarder {
    fn forward(receiver: &PacketReceiver, cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        let my_id = cluster_info.read().unwrap().id();

        loop {
            let msgs = receiver.recv()?;

            inc_new_counter_info!(
                "tpu_forwarder-msgs_received",
                msgs.read().unwrap().packets.len()
            );

            let send_addr = get_forwarding_addr(cluster_info.read().unwrap().leader_data(), &my_id);

            if let Some(send_addr) = send_addr {
                msgs.write().unwrap().set_addr(&send_addr);
                msgs.read().unwrap().send_to(&socket)?;
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
            })
            .collect();

        let thread_hdl = Builder::new()
            .name("solana-tpu_forwarder".to_string())
            .spawn(move || {
                let _ignored = Self::forward(&receiver, &cluster_info);
            })
            .unwrap();

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
    use crate::contact_info::ContactInfo;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_get_forwarding_addr() {
        let my_id = Keypair::new().pubkey();

        // Test with no leader
        assert_eq!(get_forwarding_addr(None, &my_id,), None);

        // Test with no TPU
        let leader_data = ContactInfo::default();
        assert_eq!(get_forwarding_addr(Some(&leader_data), &my_id,), None);

        // Test with my pubkey
        let leader_data = ContactInfo::new_localhost(my_id, 0);
        assert_eq!(get_forwarding_addr(Some(&leader_data), &my_id,), None);

        // Test with pubkey other than mine
        let alice_id = Keypair::new().pubkey();
        let leader_data = ContactInfo::new_localhost(alice_id, 0);
        assert!(get_forwarding_addr(Some(&leader_data), &my_id,).is_some());
    }
}
