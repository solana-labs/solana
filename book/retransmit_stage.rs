//! The `retransmit_stage` retransmits blobs between validators

use cluster_info::ClusterInfo;
use counter::Counter;
use entry::Entry;

use leader_scheduler::LeaderScheduler;
use log::Level;
use result::{Error, Result};
use service::Service;
use solana_metrics::{influxdb, submit};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::BlobReceiver;
use window::SharedWindow;
use window_service::window_service;

fn retransmit(
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    r: &BlobReceiver,
    sock: &UdpSocket,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq);
    }

    submit(
        influxdb::Point::new("retransmit-stage")
            .add_field("count", influxdb::Value::Integer(dq.len() as i64))
            .to_owned(),
    );

    for b in &mut dq {
        ClusterInfo::retransmit(&cluster_info, b, sock)?;
    }
    Ok(())
}

/// Service to retransmit messages from the leader to layer 1 nodes.
/// See `cluster_info` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to read from.  Read timeout is set to 1.
/// * `exit` - Boolean to signal system exit.
/// * `cluster_info` - This structure needs to be updated and populated by the bank and via gossip.
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
fn retransmitter(
    sock: Arc<UdpSocket>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    r: BlobReceiver,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-retransmitter".to_string())
        .spawn(move || {
            trace!("retransmitter started");
            loop {
                if let Err(e) = retransmit(&cluster_info, &r, &sock) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_info!("streamer-retransmit-error", 1, 1);
                        }
                    }
                }
            }
            trace!("exiting retransmitter");
        }).unwrap()
}

pub struct RetransmitStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl RetransmitStage {
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        tick_height: u64,
        entry_height: u64,
        retransmit_socket: Arc<UdpSocket>,
        repair_socket: Arc<UdpSocket>,
        fetch_stage_receiver: BlobReceiver,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit =
            retransmitter(retransmit_socket, cluster_info.clone(), retransmit_receiver);
        let (entry_sender, entry_receiver) = channel();
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            cluster_info.clone(),
            window,
            tick_height,
            entry_height,
            0,
            fetch_stage_receiver,
            entry_sender,
            retransmit_sender,
            repair_socket,
            leader_scheduler,
            done,
        );

        let thread_hdls = vec![t_retransmit, t_window];
        (RetransmitStage { thread_hdls }, entry_receiver)
    }
}

impl Service for RetransmitStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
