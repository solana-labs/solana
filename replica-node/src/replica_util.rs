use {
    log::*,
    rand::{seq::SliceRandom, thread_rng, Rng},
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::ContactInfo,
        gossip_service::GossipService,
    },
    solana_runtime::{snapshot_archive_info::SnapshotArchiveInfoGetter, snapshot_utils},
    solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::HashSet,
        net::{SocketAddr, UdpSocket},
        path::Path,
        process::exit,
        sync::{atomic::AtomicBool, Arc},
        thread::sleep,
        time::{Duration, Instant},
    },
};

pub fn get_cluster_shred_version(entrypoints: &[SocketAddr]) -> Option<u16> {
    let entrypoints = {
        let mut index: Vec<_> = (0..entrypoints.len()).collect();
        index.shuffle(&mut rand::thread_rng());
        index.into_iter().map(|i| &entrypoints[i])
    };
    for entrypoint in entrypoints {
        match solana_net_utils::get_cluster_shred_version(entrypoint) {
            Err(err) => eprintln!("get_cluster_shred_version failed: {}, {}", entrypoint, err),
            Ok(0) => eprintln!("zero sherd-version from entrypoint: {}", entrypoint),
            Ok(shred_version) => {
                info!(
                    "obtained shred-version {} from {}",
                    shred_version, entrypoint
                );
                return Some(shred_version);
            }
        }
    }
    None
}

// Discover the RPC peer node via Gossip and return's ContactInfo
// And the initial snapshot info: (Slot, Hash)
// Alternatively, this can be solved via a RPC call instead of using gossip.
fn get_rpc_peer_node(
    cluster_info: &ClusterInfo,
    cluster_entrypoints: &[ContactInfo],
    expected_shred_version: Option<u16>,
    peer_pubkey: &Pubkey,
    snapshot_output_dir: &Path,
) -> Option<(ContactInfo, Option<(Slot, Hash)>)> {
    let mut newer_cluster_snapshot_timeout = None;
    let mut retry_reason = None;
    loop {
        sleep(Duration::from_secs(1));
        info!("Searching for the rpc peer node and latest snapshot information with shred_version {:?}.", expected_shred_version);
        info!("\n{}", cluster_info.rpc_info_trace());

        let shred_version =
            expected_shred_version.unwrap_or_else(|| cluster_info.my_shred_version());
        if shred_version == 0 {
            let all_zero_shred_versions = cluster_entrypoints.iter().all(|cluster_entrypoint| {
                cluster_info
                    .lookup_contact_info_by_gossip_addr(&cluster_entrypoint.gossip)
                    .map_or(false, |entrypoint| entrypoint.shred_version == 0)
            });

            if all_zero_shred_versions {
                eprintln!(
                    "Entrypoint shred version is zero.  Restart with --expected-shred-version"
                );
                exit(1);
            }
            info!("Waiting to adopt entrypoint shred version...");
            continue;
        }

        info!(
            "Searching for an RPC service with shred version {}{}...",
            shred_version,
            retry_reason
                .as_ref()
                .map(|s| format!(" (Retrying: {})", s))
                .unwrap_or_default()
        );

        let rpc_peers = cluster_info
            .all_rpc_peers()
            .into_iter()
            .filter(|contact_info| contact_info.shred_version == shred_version)
            .collect::<Vec<_>>();
        let rpc_peers_total = rpc_peers.len();

        let rpc_peers_trusted = rpc_peers
            .iter()
            .filter(|rpc_peer| &rpc_peer.id == peer_pubkey)
            .count();

        info!(
            "Total {} RPC nodes found. {} trusted",
            rpc_peers_total, rpc_peers_trusted
        );

        let mut highest_snapshot_info: Option<(Slot, Hash)> =
            snapshot_utils::get_highest_full_snapshot_archive_info(snapshot_output_dir).map(
                |snapshot_archive_info| {
                    (snapshot_archive_info.slot(), *snapshot_archive_info.hash())
                },
            );
        let eligible_rpc_peers = {
            let mut eligible_rpc_peers = vec![];

            for rpc_peer in rpc_peers.iter() {
                if &rpc_peer.id != peer_pubkey {
                    continue;
                }
                cluster_info.get_snapshot_hash_for_node(&rpc_peer.id, |snapshot_hashes| {
                    for snapshot_hash in snapshot_hashes {
                        if highest_snapshot_info.is_none()
                            || snapshot_hash.0 > highest_snapshot_info.unwrap().0
                        {
                            // Found a higher snapshot, remove all nodes with a lower snapshot
                            eligible_rpc_peers.clear();
                            highest_snapshot_info = Some(*snapshot_hash)
                        }

                        if Some(*snapshot_hash) == highest_snapshot_info {
                            eligible_rpc_peers.push(rpc_peer.clone());
                        }
                    }
                });
            }

            match highest_snapshot_info {
                None => {
                    assert!(eligible_rpc_peers.is_empty());
                }
                Some(highest_snapshot_info) => {
                    if eligible_rpc_peers.is_empty() {
                        match newer_cluster_snapshot_timeout {
                            None => newer_cluster_snapshot_timeout = Some(Instant::now()),
                            Some(newer_cluster_snapshot_timeout) => {
                                if newer_cluster_snapshot_timeout.elapsed().as_secs() > 180 {
                                    warn!("giving up newer snapshot from the cluster");
                                    return None;
                                }
                            }
                        }
                        retry_reason = Some(format!(
                            "Wait for newer snapshot than local: {:?}",
                            highest_snapshot_info
                        ));
                        continue;
                    }

                    info!(
                        "Highest available snapshot slot is {}, available from {} node{}: {:?}",
                        highest_snapshot_info.0,
                        eligible_rpc_peers.len(),
                        if eligible_rpc_peers.len() > 1 {
                            "s"
                        } else {
                            ""
                        },
                        eligible_rpc_peers
                            .iter()
                            .map(|contact_info| contact_info.id)
                            .collect::<Vec<_>>()
                    );
                }
            }
            eligible_rpc_peers
        };

        if !eligible_rpc_peers.is_empty() {
            let contact_info =
                &eligible_rpc_peers[thread_rng().gen_range(0, eligible_rpc_peers.len())];
            return Some((contact_info.clone(), highest_snapshot_info));
        } else {
            retry_reason = Some("No snapshots available".to_owned());
        }
    }
}

fn start_gossip_node(
    identity_keypair: Arc<Keypair>,
    cluster_entrypoints: &[ContactInfo],
    ledger_path: &Path,
    gossip_addr: &SocketAddr,
    gossip_socket: UdpSocket,
    expected_shred_version: Option<u16>,
    gossip_validators: Option<HashSet<Pubkey>>,
    should_check_duplicate_instance: bool,
    socket_addr_space: SocketAddrSpace,
) -> (Arc<ClusterInfo>, Arc<AtomicBool>, GossipService) {
    let contact_info = ClusterInfo::gossip_contact_info(
        identity_keypair.pubkey(),
        *gossip_addr,
        expected_shred_version.unwrap_or(0),
    );
    let mut cluster_info = ClusterInfo::new(contact_info, identity_keypair, socket_addr_space);
    cluster_info.set_entrypoints(cluster_entrypoints.to_vec());
    cluster_info.restore_contact_info(ledger_path, 0);
    let cluster_info = Arc::new(cluster_info);

    let gossip_exit_flag = Arc::new(AtomicBool::new(false));
    let gossip_service = GossipService::new(
        &cluster_info,
        None,
        gossip_socket,
        gossip_validators,
        should_check_duplicate_instance,
        &gossip_exit_flag,
    );
    info!("Started gossip node");
    info!(
        "The cluster contact info:\n{}",
        cluster_info.contact_info_trace()
    );

    (cluster_info, gossip_exit_flag, gossip_service)
}

// Get the RPC peer info given the peer's Pubkey
// Returns the ClusterInfo, the peer's ContactInfo and the initial snapshot info
pub fn get_rpc_peer_info(
    identity_keypair: Keypair,
    cluster_entrypoints: &[ContactInfo],
    ledger_path: &Path,
    node: &Node,
    expected_shred_version: Option<u16>,
    peer_pubkey: &Pubkey,
    snapshot_output_dir: &Path,
    socket_addr_space: SocketAddrSpace,
) -> (Arc<ClusterInfo>, ContactInfo, Option<(Slot, Hash)>) {
    let identity_keypair = Arc::new(identity_keypair);

    let gossip = start_gossip_node(
        identity_keypair,
        cluster_entrypoints,
        ledger_path,
        &node.info.gossip,
        node.sockets.gossip.try_clone().unwrap(),
        expected_shred_version,
        None,
        true,
        socket_addr_space,
    );

    let rpc_node_details = get_rpc_peer_node(
        &gossip.0,
        cluster_entrypoints,
        expected_shred_version,
        peer_pubkey,
        snapshot_output_dir,
    );
    let rpc_node_details = rpc_node_details.unwrap();

    (gossip.0, rpc_node_details.0, rpc_node_details.1)
}
