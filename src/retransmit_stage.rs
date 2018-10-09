//! The `retransmit_stage` retransmits blobs between validators

use cluster_info::ClusterInfo;
use counter::Counter;
use entry::Entry;
use log::Level;
use result::{Error, Result};
use service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::BlobReceiver;
use window::SharedWindow;
use window_service::{window_service, WindowServiceReturnType};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RetransmitStageReturnType {
    LeaderRotation(u64),
}

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
    t_retransmit: JoinHandle<()>,
    t_window: JoinHandle<Option<WindowServiceReturnType>>,
}

impl RetransmitStage {
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window: SharedWindow,
        entry_height: u64,
        retransmit_socket: Arc<UdpSocket>,
        repair_socket: Arc<UdpSocket>,
        fetch_stage_receiver: BlobReceiver,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit =
            retransmitter(retransmit_socket, cluster_info.clone(), retransmit_receiver);
        let (entry_sender, entry_receiver) = channel();
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            cluster_info.clone(),
            window,
            entry_height,
            0,
            fetch_stage_receiver,
            entry_sender,
            retransmit_sender,
            repair_socket,
            done,
        );

        (
            RetransmitStage {
                t_window,
                t_retransmit,
            },
            entry_receiver,
        )
    }
}

impl Service for RetransmitStage {
    type JoinReturnType = Option<RetransmitStageReturnType>;

    fn join(self) -> thread::Result<Option<RetransmitStageReturnType>> {
        self.t_retransmit.join()?;
        match self.t_window.join()? {
            Some(WindowServiceReturnType::LeaderRotation(entry_height)) => Ok(Some(
                RetransmitStageReturnType::LeaderRotation(entry_height),
            )),
            _ => Ok(None),
        }
    }
}
