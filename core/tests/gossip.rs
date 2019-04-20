#[macro_use]
extern crate log;

use rayon::iter::*;
use solana::cluster_info::{ClusterInfo, Node};
use solana::gossip_service::GossipService;

use solana::packet::{Blob, SharedBlob};
use solana::result;
use solana::service::Service;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

fn test_node(exit: &Arc<AtomicBool>) -> (Arc<RwLock<ClusterInfo>>, GossipService, UdpSocket) {
    let keypair = Arc::new(Keypair::new());
    let mut test_node = Node::new_localhost_with_pubkey(&keypair.pubkey());
    let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(
        test_node.info.clone(),
        keypair,
    )));
    let gossip_service =
        GossipService::new(&cluster_info, None, None, test_node.sockets.gossip, exit);
    let _ = cluster_info.read().unwrap().my_data();
    (
        cluster_info,
        gossip_service,
        test_node.sockets.tvu.pop().unwrap(),
    )
}

/// Test that the network converges.
/// Run until every node in the network has a full ContactInfo set.
/// Check that nodes stop sending updates after all the ContactInfo has been shared.
/// tests that actually use this function are below
fn run_gossip_topo<F>(num: usize, topo: F)
where
    F: Fn(&Vec<(Arc<RwLock<ClusterInfo>>, GossipService, UdpSocket)>) -> (),
{
    let exit = Arc::new(AtomicBool::new(false));
    let listen: Vec<_> = (0..num).map(|_| test_node(&exit)).collect();
    topo(&listen);
    let mut done = true;
    for i in 0..(num * 32) {
        done = true;
        let total: usize = listen
            .iter()
            .map(|v| v.0.read().unwrap().gossip_peers().len())
            .sum();
        if (total + num) * 10 > num * num * 9 {
            done = true;
            break;
        } else {
            trace!("not converged {} {} {}", i, total + num, num * num);
        }
        sleep(Duration::new(1, 0));
    }
    exit.store(true, Ordering::Relaxed);
    for (_, dr, _) in listen {
        dr.join().unwrap();
    }
    assert!(done);
}
/// ring a -> b -> c -> d -> e -> a
#[test]
fn gossip_ring() -> result::Result<()> {
    solana_logger::setup();
    run_gossip_topo(50, |listen| {
        let num = listen.len();
        for n in 0..num {
            let y = n % listen.len();
            let x = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut d = yv.lookup(&yv.id()).unwrap().clone();
            d.wallclock = timestamp();
            xv.insert_info(d);
        }
    });

    Ok(())
}

/// ring a -> b -> c -> d -> e -> a
#[test]
#[ignore]
fn gossip_ring_large() -> result::Result<()> {
    solana_logger::setup();
    run_gossip_topo(600, |listen| {
        let num = listen.len();
        for n in 0..num {
            let y = n % listen.len();
            let x = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut d = yv.lookup(&yv.id()).unwrap().clone();
            d.wallclock = timestamp();
            xv.insert_info(d);
        }
    });

    Ok(())
}
/// star a -> (b,c,d,e)
#[test]
fn gossip_star() {
    solana_logger::setup();
    run_gossip_topo(10, |listen| {
        let num = listen.len();
        for n in 0..(num - 1) {
            let x = 0;
            let y = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut yd = yv.lookup(&yv.id()).unwrap().clone();
            yd.wallclock = timestamp();
            xv.insert_info(yd);
            trace!("star leader {}", &xv.id());
        }
    });
}

/// rstar a <- (b,c,d,e)
#[test]
fn gossip_rstar() {
    solana_logger::setup();
    run_gossip_topo(10, |listen| {
        let num = listen.len();
        let xd = {
            let xv = listen[0].0.read().unwrap();
            xv.lookup(&xv.id()).unwrap().clone()
        };
        trace!("rstar leader {}", xd.id);
        for n in 0..(num - 1) {
            let y = (n + 1) % listen.len();
            let mut yv = listen[y].0.write().unwrap();
            yv.insert_info(xd.clone());
            trace!("rstar insert {} into {}", xd.id, yv.id());
        }
    });
}

#[test]
pub fn cluster_info_retransmit() -> result::Result<()> {
    solana_logger::setup();
    let exit = Arc::new(AtomicBool::new(false));
    trace!("c1:");
    let (c1, dr1, tn1) = test_node(&exit);
    trace!("c2:");
    let (c2, dr2, tn2) = test_node(&exit);
    trace!("c3:");
    let (c3, dr3, tn3) = test_node(&exit);
    let c1_data = c1.read().unwrap().my_data().clone();

    c2.write().unwrap().insert_info(c1_data.clone());
    c3.write().unwrap().insert_info(c1_data.clone());

    let num = 3;

    //wait to converge
    trace!("waiting to converge:");
    let mut done = false;
    for _ in 0..30 {
        done = c1.read().unwrap().gossip_peers().len() == num - 1
            && c2.read().unwrap().gossip_peers().len() == num - 1
            && c3.read().unwrap().gossip_peers().len() == num - 1;
        if done {
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(done);
    let b = SharedBlob::default();
    b.write().unwrap().meta.size = 10;
    ClusterInfo::retransmit(&c1, &b, &tn1, false)?;
    let res: Vec<_> = [tn1, tn2, tn3]
        .into_par_iter()
        .map(|s| {
            let mut b = Blob::default();
            s.set_read_timeout(Some(Duration::new(1, 0))).unwrap();
            let res = s.recv_from(&mut b.data);
            res.is_err() //true if failed to receive the retransmit packet
        })
        .collect();
    //true if failed receive the retransmit packet, r2, and r3 should succeed
    //r1 was the sender, so it should fail to receive the packet
    assert_eq!(res, [true, false, false]);
    exit.store(true, Ordering::Relaxed);
    dr1.join().unwrap();
    dr2.join().unwrap();
    dr3.join().unwrap();

    Ok(())
}
