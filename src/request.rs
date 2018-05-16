//! The `request` module defines the messages for the thin client.

use bincode::serialize;
use hash::Hash;
use packet;
use packet::SharedPackets;
use serde::Serialize;
use signature::PublicKey;

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    GetBalance { key: PublicKey },
    GetLastId,
    GetTransactionCount,
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        true
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance { key: PublicKey, val: Option<i64> },
    LastId { id: Hash },
    TransactionCount { transaction_count: u64 },
}

pub fn to_request_packets<T: Serialize>(
    r: &packet::PacketRecycler,
    reqs: Vec<T>,
) -> Vec<SharedPackets> {
    let mut out = vec![];
    for rrs in reqs.chunks(packet::NUM_PACKETS) {
        let p = r.allocate();
        p.write()
            .unwrap()
            .packets
            .resize(rrs.len(), Default::default());
        for (i, o) in rrs.iter().zip(p.write().unwrap().packets.iter_mut()) {
            let v = serialize(&i).expect("serialize request");
            let len = v.len();
            o.data[..len].copy_from_slice(&v);
            o.meta.size = len;
        }
        out.push(p);
    }
    return out;
}

#[cfg(test)]
mod tests {
    use packet::{PacketRecycler, NUM_PACKETS};
    use request::{to_request_packets, Request};

    #[test]
    fn test_to_packets() {
        let tr = Request::GetTransactionCount;
        let re = PacketRecycler::default();
        let rv = to_request_packets(&re, vec![tr.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), 1);

        let rv = to_request_packets(&re, vec![tr.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);

        let rv = to_request_packets(&re, vec![tr.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].read().unwrap().packets.len(), 1);
    }
}
