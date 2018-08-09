use crdt::NodeInfo;
use influx_db_client as influxdb;
use metrics;
use signature::{GenKeys, KeyPair, KeyPairUtil, PublicKey};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thin_client::ThinClient;
use timing::{duration_as_ms, duration_as_s};
use transaction::Transaction;

const UPDATE_MILLIS: u64 = 500;

/// execute transfers between keys
/// 1. `tx_count` keys are generated
/// 2. each key is given the `amount` of tokens
/// 3. first half of keys sends 1 token to second half of keys
/// 4. and vice versa
/// 5. since some of the transactions may get dropped, the `amount` of tokens should cover the
///    unbalanced drops between the halfs
pub fn exececute(
    leader: &NodeInfo,
    client: &mut ThinClient,
    source: &KeyPair,
    tx_count: usize,
    amount: i64,
    exit_signal: Arc<AtomicBool>,
) {
    info!("Creating {} keypairs...", tx_count);
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&source.public_key_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);
    let keys = rnd.gen_n_keypairs(tx_count as i64);
    //each key will have a RESVER_AMOUNT of tokens
    distribute(client, source, &keys, amount);

    while !exit_signal.load(Ordering::Relaxed) {
        let transfer_start = Instant::now();
        info!(
            "Transferring 1 unit {} times... to {}",
            tx_count, leader.contact_info.tpu
        );

        let half = tx_count / 2;
        //each key will have a RESERVE_AMOUNT of tokens
        //and can swap 1 token, allow for up to RESERVE_AMOUNT of unbalanced drops
        swap(client, &keys[..half], &keys[half..]);
        swap(client, &keys[half..], &keys[..half]);

        info!(
            "Tx send done. {} ms {} tps",
            duration_as_ms(&transfer_start.elapsed()),
            tx_count as f32 / duration_as_s(&transfer_start.elapsed()),
        );
        metrics::submit(
            influxdb::Point::new("bench-tps")
                .add_tag("op", influxdb::Value::String("do_tx_transfers".to_string()))
                .add_field(
                    "duration",
                    influxdb::Value::Integer(duration_as_ms(&transfer_start.elapsed()) as i64),
                )
                .add_field("count", influxdb::Value::Integer(tx_count as i64))
                .to_owned(),
        );
    }
    reclaim(client, &keys, &source.pubkey());
}

pub fn distribute(client: &mut ThinClient, source: &KeyPair, keypairs: &[KeyPair], amount: i64) {
    // distribute initial seed to the group
    let desired = amount * (keypairs.len() as i64);
    loop {
        let last_id = client.get_last_id();
        let tx = Transaction::new(source, keypairs[0].pubkey(), desired, last_id);
        client.transfer_signed(&tx).unwrap();
        let _ = client
            .poll_update(30 * UPDATE_MILLIS, &keypairs[0].pubkey())
            .expect("initial seed transfer");
        let bal = client.get_balance(&keypairs[0].pubkey()).unwrap_or(0);
        assert!(bal == 0 || bal == desired);
        if bal == desired {
            break;
        }
    }

    let mut distributed = 0usize;
    // distribute within the group
    while distributed < keypairs.len() {
        distributed = 0;
        let last_id = client.get_last_id();
        let sources: Vec<&KeyPair> = keypairs
            .iter()
            .filter_map(|key| {
                let balance = client.get_balance(&key.pubkey()).unwrap_or(0);
                if balance > amount {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        let destinations: Vec<&KeyPair> = keypairs
            .iter()
            .filter_map(|key| {
                let balance = client.get_balance(&key.pubkey()).unwrap_or(0);
                if balance > 0 {
                    None
                } else {
                    Some(key)
                }
            })
            .collect();
        sources.iter().zip(destinations.iter()).for_each(|(s, d)| {
            let balance = client.get_balance(&s.pubkey()).unwrap();
            let spend = ((balance / amount) / 2) * amount;
            let tx = Transaction::new(s, d.pubkey(), spend, last_id.clone());
            client.transfer_signed(&tx).unwrap();
        });
        let count: usize = sources
            .iter()
            .map(|s| {
                let _ = client.poll_update(UPDATE_MILLIS, &s.pubkey());
                let bal = client.get_balance(&s.pubkey()).unwrap_or(0);
                (bal > 0) as usize
            })
            .sum();
        distributed += count;
        let count: usize = destinations
            .iter()
            .map(|s| {
                let _ = client.poll_update(UPDATE_MILLIS, &s.pubkey());
                let bal = client.get_balance(&s.pubkey()).unwrap_or(0);
                (bal > 0) as usize
            })
            .sum();
        distributed += count;
    }
}

pub fn swap(client: &mut ThinClient, sources: &[KeyPair], destinations: &[KeyPair]) {
    let last_id = client.get_last_id();
    sources.iter().zip(destinations.iter()).for_each(|(s, d)| {
        let tx = Transaction::new(s, d.pubkey(), 1, last_id.clone());
        client.transfer_signed(&tx).unwrap();
    });
}

pub fn reclaim(client: &mut ThinClient, keypairs: &[KeyPair], sink: &PublicKey) {
    let mut unreclaimed = keypairs.len();
    while unreclaimed != 0 {
        let last_id = client.get_last_id();
        let sources: Vec<&KeyPair> = keypairs
            .iter()
            .filter_map(|key| {
                let balance = client.get_balance(&key.pubkey()).unwrap_or(0);
                if balance > 0 {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        sources.iter().for_each(|s| {
            let spend = client.get_balance(&s.pubkey()).unwrap();
            let tx = Transaction::new(s, *sink, spend, last_id.clone());
            client.transfer_signed(&tx).unwrap();
        });
        unreclaimed = sources
            .iter()
            .map(|s| {
                let spend = client.get_balance(&s.pubkey()).unwrap();
                if spend == 0 {
                    0
                } else {
                    let _ = client.poll_update(UPDATE_MILLIS, &s.pubkey());
                    let bal = client.get_balance(&s.pubkey()).unwrap_or(0);
                    (bal > 0) as usize
                }
            })
            .sum();
    }
}
