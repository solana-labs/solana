#[macro_use]
extern crate log;
extern crate rayon;
extern crate solana;

use rayon::iter::*;
use solana::cluster_info::{ClusterInfo, Node};
use solana::logger;
use solana::ncp::Ncp;
use solana::packet::{Blob, SharedBlob};
use solana::result;
use solana::service::Service;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

fn test_node(exit: Arc<AtomicBool>) -> (Arc<RwLock<ClusterInfo>>, Ncp, UdpSocket) {
    let mut tn = Node::new_localhost();
    let cluster_info = ClusterInfo::new(tn.info.clone()).expect("ClusterInfo::new");
    let c = Arc::new(RwLock::new(cluster_info));
    let w = Arc::new(RwLock::new(vec![]));
    let d = Ncp::new(&c.clone(), w, None, tn.sockets.gossip, exit);
    (c, d, tn.sockets.replicate.pop().unwrap())
}

/// Test that the network converges.
/// Run until every node in the network has a full NodeInfo set.
/// Check that nodes stop sending updates after all the NodeInfo has been shared.
/// tests that actually use this function are below
fn run_gossip_topo<F>(topo: F)
where
    F: Fn(&Vec<(Arc<RwLock<ClusterInfo>>, Ncp, UdpSocket)>) -> (),
{
    let num: usize = 5;
    let exit = Arc::new(AtomicBool::new(false));
    let listen: Vec<_> = (0..num).map(|_| test_node(exit.clone())).collect();
    topo(&listen);
    let mut done = true;
    for i in 0..(num * 32) {
        done = false;
        trace!("round {}", i);
        for (c, _, _) in &listen {
            if num == c.read().unwrap().convergence() as usize {
                done = true;
                break;
            }
        }
        //at least 1 node converged
        if done == true {
            break;
        }
        sleep(Duration::new(1, 0));
    }
    exit.store(true, Ordering::Relaxed);
    for (c, dr, _) in listen {
        dr.join().unwrap();
        // make it clear what failed
        // protocol is to chatty, updates should stop after everyone receives `num`
        assert!(c.read().unwrap().update_index <= num as u64);
        // protocol is not chatty enough, everyone should get `num` entries
        assert_eq!(c.read().unwrap().table.len(), num);
    }
    assert!(done);
}
/// ring a -> b -> c -> d -> e -> a
#[test]
fn gossip_ring() -> result::Result<()> {
    logger::setup();
    run_gossip_topo(|listen| {
        let num = listen.len();
        for n in 0..num {
            let y = n % listen.len();
            let x = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut d = yv.table[&yv.id].clone();
            d.version = 0;
            xv.insert(&d);
        }
    });

    Ok(())
}

/// star a -> (b,c,d,e)
#[test]
fn gossip_star() {
    logger::setup();
    run_gossip_topo(|listen| {
        let num = listen.len();
        for n in 0..(num - 1) {
            let x = 0;
            let y = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut yd = yv.table[&yv.id].clone();
            yd.version = 0;
            xv.insert(&yd);
            trace!("star leader {:?}", &xv.id.as_ref()[..4]);
        }
    });
}

/// rstar a <- (b,c,d,e)
#[test]
fn gossip_rstar() {
    logger::setup();
    run_gossip_topo(|listen| {
        let num = listen.len();
        let xd = {
            let xv = listen[0].0.read().unwrap();
            xv.table[&xv.id].clone()
        };
        trace!("rstar leader {:?}", &xd.id.as_ref()[..4]);
        for n in 0..(num - 1) {
            let y = (n + 1) % listen.len();
            let mut yv = listen[y].0.write().unwrap();
            yv.insert(&xd);
            trace!(
                "rstar insert {:?} into {:?}",
                &xd.id.as_ref()[..4],
                &yv.id.as_ref()[..4]
            );
        }
    });
}

#[test]
pub fn cluster_info_retransmit() -> result::Result<()> {
    logger::setup();
    let exit = Arc::new(AtomicBool::new(false));
    trace!("c1:");
    let (c1, dr1, tn1) = test_node(exit.clone());
    trace!("c2:");
    let (c2, dr2, tn2) = test_node(exit.clone());
    trace!("c3:");
    let (c3, dr3, tn3) = test_node(exit.clone());
    let c1_data = c1.read().unwrap().my_data().clone();
    c1.write().unwrap().set_leader(c1_data.id);

    c2.write().unwrap().insert(&c1_data);
    c3.write().unwrap().insert(&c1_data);

    c2.write().unwrap().set_leader(c1_data.id);
    c3.write().unwrap().set_leader(c1_data.id);

    //wait to converge
    trace!("waiting to converge:");
    let mut done = false;
    for _ in 0..30 {
        done = c1.read().unwrap().table.len() == 3
            && c2.read().unwrap().table.len() == 3
            && c3.read().unwrap().table.len() == 3;
        if done {
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(done);
    let b = SharedBlob::default();
    b.write().unwrap().meta.size = 10;
    ClusterInfo::retransmit(&c1, &b, &tn1)?;
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

#[test]
#[ignore]
fn test_external_liveness_table() {
    logger::setup();
    assert!(cfg!(feature = "test"));
    let c1_c4_exit = Arc::new(AtomicBool::new(false));
    let c2_c3_exit = Arc::new(AtomicBool::new(false));

    trace!("c1:");
    let (c1, dr1, _) = test_node(c1_c4_exit.clone());
    trace!("c2:");
    let (c2, dr2, _) = test_node(c2_c3_exit.clone());
    trace!("c3:");
    let (c3, dr3, _) = test_node(c2_c3_exit.clone());
    trace!("c4:");
    let (c4, dr4, _) = test_node(c1_c4_exit.clone());

    let c1_data = c1.read().unwrap().my_data().clone();
    c1.write().unwrap().set_leader(c1_data.id);

    let c2_id = c2.read().unwrap().id;
    let c3_id = c3.read().unwrap().id;
    let c4_id = c4.read().unwrap().id;

    // Insert the remote data about c4
    let c2_index_for_c4 = 10;
    c2.write().unwrap().remote.insert(c4_id, c2_index_for_c4);
    let c3_index_for_c4 = 20;
    c3.write().unwrap().remote.insert(c4_id, c3_index_for_c4);

    // Set up the initial network topology
    c2.write().unwrap().insert(&c1_data);
    c3.write().unwrap().insert(&c1_data);

    c2.write().unwrap().set_leader(c1_data.id);
    c3.write().unwrap().set_leader(c1_data.id);

    // Wait to converge
    trace!("waiting to converge:");
    let mut done = false;
    for _ in 0..30 {
        done = c1.read().unwrap().table.len() == 3
            && c2.read().unwrap().table.len() == 3
            && c3.read().unwrap().table.len() == 3;
        if done {
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(done);

    // Validate c1's external liveness table, then release lock rc1
    {
        let rc1 = c1.read().unwrap();
        let el = rc1.get_external_liveness_entry(&c4.read().unwrap().id);

        // Make sure liveness table entry for c4 exists on node c1
        assert!(el.is_some());
        let liveness_map = el.unwrap();

        // Make sure liveness table entry contains correct result for c2
        let c2_index_result_for_c4 = liveness_map.get(&c2_id);
        assert!(c2_index_result_for_c4.is_some());
        assert_eq!(*(c2_index_result_for_c4.unwrap()), c2_index_for_c4);

        // Make sure liveness table entry contains correct result for c3
        let c3_index_result_for_c4 = liveness_map.get(&c3_id);
        assert!(c3_index_result_for_c4.is_some());
        assert_eq!(*(c3_index_result_for_c4.unwrap()), c3_index_for_c4);
    }

    // Shutdown validators c2 and c3
    c2_c3_exit.store(true, Ordering::Relaxed);
    dr2.join().unwrap();
    dr3.join().unwrap();

    // Allow communication between c1 and c4, make sure that c1's external_liveness table
    // entry for c4 gets cleared
    c4.write().unwrap().insert(&c1_data);
    c4.write().unwrap().set_leader(c1_data.id);
    for _ in 0..30 {
        done = c1
            .read()
            .unwrap()
            .get_external_liveness_entry(&c4_id)
            .is_none();
        if done {
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(done);

    // Shutdown validators c1 and c4
    c1_c4_exit.store(true, Ordering::Relaxed);
    dr1.join().unwrap();
    dr4.join().unwrap();
}
