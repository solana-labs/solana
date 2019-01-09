//! The `tpu_forwarder` module implements a validator's
//!  transaction processing unit responsibility, which
//!  forwards received packets to the current leader

use crate::cluster_info::ClusterInfo;
use crate::contact_info::ContactInfo;
use crate::counter::Counter;
use crate::packet::Packets;
use crate::result::Result;
use crate::service::Service;
use crate::streamer::{self, PacketReceiver};
use log::Level;
use solana_sdk::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};

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
            let leader_data = cluster_info
                .read()
                .expect("cluster_info.read() in TpuForwarder::forward()")
                .leader_data()
                .cloned();
            if TpuForwarder::update_addrs(leader_data, &my_id, msgs.clone()) {
                msgs.read().unwrap().send_to(&socket)?
            }
            continue;
        }
    }

    fn update_addrs(
        leader_data: Option<ContactInfo>,
        my_id: &Pubkey,
        msgs: Arc<RwLock<Packets>>,
    ) -> bool {
        match leader_data {
            Some(leader_data) => {
                if leader_data.id == *my_id || !ContactInfo::is_valid_address(&leader_data.tpu) {
                    // weird cases, but we don't want to broadcast, send to ANY, or
                    // induce an infinite loop, but this shouldn't happen, or shouldn't be true for long...
                    return false;
                }

                for m in msgs.write().unwrap().packets.iter_mut() {
                    m.meta.set_addr(&leader_data.tpu);
                }
                true
            }
            _ => false,
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
    use crate::packet::Packet;
    use solana_netutil::bind_in_range;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    pub fn test_update_addrs() {
        let keypair = Keypair::new();
        let my_id = keypair.pubkey();
        // test with no pubkey
        assert!(!TpuForwarder::update_addrs(
            None,
            &my_id,
            Arc::new(RwLock::new(Packets::default()))
        ));
        // test with no tvu
        assert!(!TpuForwarder::update_addrs(
            Some(ContactInfo::default()),
            &my_id,
            Arc::new(RwLock::new(Packets::default()))
        ));
        // test with my pubkey
        let leader_data = ContactInfo::new_localhost(my_id, 0);
        assert!(!TpuForwarder::update_addrs(
            Some(leader_data),
            &my_id,
            Arc::new(RwLock::new(Packets::default()))
        ));
        // test that the address is actually being updated
        let (port, _) = bind_in_range((8000, 10000)).unwrap();
        let leader_data = ContactInfo::new_with_socketaddr(&socketaddr!([127, 0, 0, 1], port));
        let packet = Packet::default();
        let p = Packets {
            packets: vec![packet],
        };
        let msgs = Arc::new(RwLock::new(p));
        assert!(TpuForwarder::update_addrs(
            Some(leader_data.clone()),
            &my_id,
            msgs.clone()
        ));
        assert_eq!(
            SocketAddr::from(msgs.read().unwrap().packets[0].meta.addr()),
            leader_data.tpu
        );
    }
}
