//! The `subscribers` module defines data structures to keep track of nodes on the network.
//! The network is arranged in layers:
//!
//! * layer 0 - Leader.
//! * layer 1 - As many nodes as we can fit to quickly get reliable `2/3+1` finality
//! * layer 2 - Everyone else, if layer 1 is `2^10`, layer 2 should be able to fit `2^20` number of nodes.
//!
//! It's up to the external state machine to keep this updated.
use packet::Blob;
use rayon::prelude::*;
use result::{Error, Result};
use std::net::{SocketAddr, UdpSocket};

#[derive(Clone, PartialEq)]
pub struct Node {
    pub id: [u64; 8],
    pub weight: u64,
    pub addr: SocketAddr,
}

//sockaddr doesn't implement default
impl Default for Node {
    fn default() -> Node {
        Node {
            id: [0; 8],
            weight: 0,
            addr: "0.0.0.0:0".parse().unwrap(),
        }
    }
}

impl Node {
    pub fn new(id: [u64; 8], weight: u64, addr: SocketAddr) -> Node {
        Node { id, weight, addr }
    }
    fn key(&self) -> i64 {
        (self.weight as i64).checked_neg().unwrap()
    }
}

pub struct Subscribers {
    data: Vec<Node>,
    pub me: Node,
    pub leader: Node,
}

impl Subscribers {
    pub fn new(me: Node, leader: Node, network: &[Node]) -> Subscribers {
        let mut h = Subscribers {
            data: vec![],
            me: me.clone(),
            leader: leader.clone(),
        };
        h.insert(&[me, leader]);
        h.insert(network);
        h
    }

    /// retransmit messages from the leader to layer 1 nodes
    pub fn retransmit(&self, blob: &mut Blob, s: &UdpSocket) -> Result<()> {
        let errs: Vec<_> = self.data
            .par_iter()
            .map(|i| {
                if self.me == *i {
                    return Ok(0);
                }
                if self.leader == *i {
                    return Ok(0);
                }
                trace!("retransmit blob to {}", i.addr);
                s.send_to(&blob.data[..blob.meta.size], &i.addr)
            })
            .collect();
        for e in errs {
            trace!("retransmit result {:?}", e);
            match e {
                Err(e) => return Err(Error::IO(e)),
                _ => (),
            }
        }
        Ok(())
    }
    pub fn insert(&mut self, ns: &[Node]) {
        self.data.extend_from_slice(ns);
        self.data.sort_by_key(Node::key);
    }
}

#[cfg(test)]
mod test {
    use packet::Blob;
    use rayon::prelude::*;
    use std::net::UdpSocket;
    use std::time::Duration;
    use subscribers::{Node, Subscribers};

    #[test]
    pub fn subscriber() {
        let mut me = Node::default();
        me.weight = 10;
        let mut leader = Node::default();
        leader.weight = 11;
        let mut s = Subscribers::new(me, leader, &[]);
        assert_eq!(s.data.len(), 2);
        assert_eq!(s.data[0].weight, 11);
        assert_eq!(s.data[1].weight, 10);
        let mut n = Node::default();
        n.weight = 12;
        s.insert(&[n]);
        assert_eq!(s.data.len(), 3);
        assert_eq!(s.data[0].weight, 12);
    }
    #[test]
    pub fn retransmit() {
        let s1 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let s2 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let s3 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let n1 = Node::new([0; 8], 0, s1.local_addr().unwrap());
        let n2 = Node::new([0; 8], 0, s2.local_addr().unwrap());
        let mut s = Subscribers::new(n1.clone(), n2.clone(), &[]);
        let n3 = Node::new([0; 8], 0, s3.local_addr().unwrap());
        s.insert(&[n3]);
        let mut b = Blob::default();
        b.meta.size = 10;
        let s4 = UdpSocket::bind("127.0.0.1:0").expect("bind");
        s.retransmit(&mut b, &s4).unwrap();
        let res: Vec<_> = [s1, s2, s3]
            .into_par_iter()
            .map(|s| {
                let mut b = Blob::default();
                s.set_read_timeout(Some(Duration::new(1, 0))).unwrap();
                s.recv_from(&mut b.data).is_err()
            })
            .collect();
        assert_eq!(res, [true, true, false]);
        let mut n4 = Node::default();
        n4.addr = "255.255.255.255:1".parse().unwrap();
        s.insert(&[n4]);
        assert!(s.retransmit(&mut b, &s4).is_err());
    }
}
