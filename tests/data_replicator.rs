#[macro_use]
extern crate log;
extern crate rayon;
extern crate solana;

use rayon::iter::*;
use solana::crdt::{Crdt, TestNode};
use solana::data_replicator::DataReplicator;
use solana::logger;
use solana::packet::Blob;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

fn test_node(exit: Arc<AtomicBool>) -> (Arc<RwLock<Crdt>>, DataReplicator, UdpSocket) {
    let tn = TestNode::new();
    let crdt = Crdt::new(tn.data.clone());
    let c = Arc::new(RwLock::new(crdt));
    let w = Arc::new(RwLock::new(vec![]));
    let d = DataReplicator::new(
        c.clone(),
        w,
        tn.sockets.gossip,
        tn.sockets.gossip_send,
        exit,
    ).unwrap();
    (c, d, tn.sockets.replicate)
}

/// Test that the network converges.
/// Run until every node in the network has a full ReplicatedData set.
/// Check that nodes stop sending updates after all the ReplicatedData has been shared.
/// tests that actually use this function are below
fn run_gossip_topo<F>(topo: F)
where
    F: Fn(&Vec<(Arc<RwLock<Crdt>>, DataReplicator, UdpSocket)>) -> (),
{
    let num: usize = 5;
    let exit = Arc::new(AtomicBool::new(false));
    let listen: Vec<_> = (0..num).map(|_| test_node(exit.clone())).collect();
    topo(&listen);
    let mut done = true;
    for i in 0..(num * 32) {
        done = false;
        trace!("round {}", i);
        for &(ref c, _, _) in listen.iter() {
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
    for (c, dr, _) in listen.into_iter() {
        for j in dr.thread_hdls.into_iter() {
            j.join().unwrap();
        }
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
fn gossip_ring() {
    logger::setup();
    run_gossip_topo(|listen| {
        let num = listen.len();
        for n in 0..num {
            let y = n % listen.len();
            let x = (n + 1) % listen.len();
            let mut xv = listen[x].0.write().unwrap();
            let yv = listen[y].0.read().unwrap();
            let mut d = yv.table[&yv.me].clone();
            d.version = 0;
            xv.insert(&d);
        }
    });
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
            let mut yd = yv.table[&yv.me].clone();
            yd.version = 0;
            xv.insert(&yd);
            trace!("star leader {:?}", &xv.me[..4]);
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
            xv.table[&xv.me].clone()
        };
        trace!("rstar leader {:?}", &xd.id[..4]);
        for n in 0..(num - 1) {
            let y = (n + 1) % listen.len();
            let mut yv = listen[y].0.write().unwrap();
            yv.insert(&xd);
            trace!("rstar insert {:?} into {:?}", &xd.id[..4], &yv.me[..4]);
        }
    });
}

#[test]
pub fn crdt_retransmit() {
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
    let mut b = Blob::default();
    b.meta.size = 10;
    Crdt::retransmit(&c1, &Arc::new(RwLock::new(b)), &tn1).unwrap();
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
    let mut threads = vec![];
    threads.extend(dr1.thread_hdls.into_iter());
    threads.extend(dr2.thread_hdls.into_iter());
    threads.extend(dr3.thread_hdls.into_iter());
    for t in threads.into_iter() {
        t.join().unwrap();
    }
}
