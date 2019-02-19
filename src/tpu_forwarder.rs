//! The `tpu_forwarder` module implements a validator's
//!  transaction processing unit responsibility, which
//!  forwards received packets to the current leader

use crate::banking_stage::UnprocessedPackets;
use crate::cluster_info::ClusterInfo;
use crate::contact_info::ContactInfo;
use crate::service::Service;
use crate::streamer::{self, PacketReceiver};
use log::Level;
use solana_metrics::counter::Counter;
use solana_sdk::pubkey::Pubkey;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};

fn get_forwarding_addr(leader_data: Option<&ContactInfo>, my_id: &Pubkey) -> Option<SocketAddr> {
    let leader_data = leader_data?;
    if leader_data.id == *my_id {
        info!("I may be stuck in a loop"); // Should never try to forward to ourselves
        return None;
    }
    if !ContactInfo::is_valid_address(&leader_data.tpu) {
        return None;
    }
    Some(leader_data.tpu)
}

pub struct TpuForwarder {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
    forwarder_thread: Option<JoinHandle<UnprocessedPackets>>,
}

impl TpuForwarder {
    fn forward(
        receiver: &PacketReceiver,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        exit: &Arc<AtomicBool>,
    ) -> UnprocessedPackets {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Unable to bind");
        let my_id = cluster_info.read().unwrap().id();

        let mut unprocessed_packets = vec![];
        loop {
            match receiver.recv() {
                Ok(msgs) => {
                    inc_new_counter_info!(
                        "tpu_forwarder-msgs_received",
                        msgs.read().unwrap().packets.len()
                    );

                    if exit.load(Ordering::Relaxed) {
                        // Collect all remaining packets on exit signaled
                        unprocessed_packets.push((msgs, 0));
                        continue;
                    }

                    match get_forwarding_addr(cluster_info.read().unwrap().leader_data(), &my_id) {
                        Some(send_addr) => {
                            msgs.write().unwrap().set_addr(&send_addr);
                            msgs.read().unwrap().send_to(&socket).unwrap_or_else(|err| {
                                info!("Failed to forward packet to {:?}: {:?}", send_addr, err)
                            });
                        }
                        None => warn!("Packets dropped due to no forwarding address"),
                    }
                }
                Err(err) => {
                    trace!("Exiting forwarder due to {:?}", err);
                    break;
                }
            }
        }
        unprocessed_packets
    }

    pub fn join_and_collect_unprocessed_packets(&mut self) -> UnprocessedPackets {
        let forwarder_thread = self.forwarder_thread.take().unwrap();
        forwarder_thread.join().unwrap_or_else(|err| {
            warn!("forwarder_thread join failed: {:?}", err);
            vec![]
        })
    }

    pub fn new(sockets: Vec<UdpSocket>, cluster_info: Arc<RwLock<ClusterInfo>>) -> Self {
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();

        let thread_hdls: Vec<_> = sockets
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

        let thread_exit = exit.clone();
        let forwarder_thread = Some(
            Builder::new()
                .name("solana-tpu_forwarder".to_string())
                .spawn(move || Self::forward(&receiver, &cluster_info, &thread_exit))
                .unwrap(),
        );

        TpuForwarder {
            exit,
            thread_hdls,
            forwarder_thread,
        }
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
        if let Some(forwarder_thread) = self.forwarder_thread {
            forwarder_thread.join()?;
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
