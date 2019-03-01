use crate::client::mk_client;
use crate::contact_info::ContactInfo;
use crate::gossip_service::converge;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;

/// Spend and verify from every node in the network
pub fn spend_and_verify_all_nodes(entry: &ContactInfo, alice: &Keypair, nodes: usize) {
    let network = converge(&entry, nodes);
    assert!(network.len() >= nodes);
    for n in &network {
        let bob = Keypair::new();
        let mut client = mk_client(&n);
        let bal = client
            .poll_get_balance(&alice.pubkey())
            .expect("balance in source");
        assert!(bal > 0);
        let mut transaction =
            SystemTransaction::new_move(&alice, bob.pubkey(), 1, client.get_last_id(), 0);
        let sig = client.retry_transfer(&alice, &mut transaction, 5).unwrap();
        for j in &network {
            let mut client = mk_client(&j);
            client.poll_for_signature(&sig).unwrap();
        }
    }
}
