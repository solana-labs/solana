use {
    rand::{CryptoRng, Rng},
    solana_client::{rpc_client::RpcClient, rpc_response::RpcContactInfo},
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::{ContactInfo, LegacyContactInfo},
        crds_gossip::CrdsGossip,
        crds_gossip_pull::CrdsFilter,
        crds_value::{self, CrdsData, CrdsValue, NodeInstance},
        gossip_service::{
            discover, discover_cluster, make_beacon_node, make_gossip_node, GossipService,
        },
    },
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::{
        client,
        packet::Packet,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        timing::timestamp,
    },
    solana_streamer::{
        packet::{PacketBatch, PacketBatchRecycler},
        sendmmsg::batch_send,
        socket::SocketAddrSpace,
    },
    solana_version::Version,
    std::{
        collections::HashSet,
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

fn get_devnet_entrypoint_keys() -> HashSet<Pubkey> {
    let mut keys = HashSet::new();
    keys.insert(Pubkey::from_str("9zi76mvDyzPPWx7Dg32hTGVhvCDVzv9X7H13QPG5nGfq").unwrap());
    keys.insert(Pubkey::from_str("dv3qDFk1DTF36Z62bNvrCXe9sKATA6xvVy6A798xxAS").unwrap());
    keys.insert(Pubkey::from_str("dv2eQHeP4RFrJZ6UeiZWoc3XTtmtZCUKxxCApCDcRNV").unwrap());
    keys
}

fn get_mainnet_entrypoint_keys() -> HashSet<Pubkey> {
    let mut keys = HashSet::new();
    keys.insert(Pubkey::from_str("7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2").unwrap());
    keys.insert(Pubkey::from_str("XLMPQY5KgC945dUiN2jAKfd7gpMmDqEVkPayPsCbVdM").unwrap());
    keys.insert(Pubkey::from_str("GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ").unwrap());
    keys
}
fn get_devnet_entrypoint_socket_addr() -> SocketAddr {
    return SocketAddr::from(([35, 197, 53, 105], 8001));
}

fn get_mainnet_entrypoint_socket_addr() -> SocketAddr {
    return SocketAddr::from(([145, 40, 67, 83], 8001));
}

fn node_emu() {
    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let keypair = Keypair::new();

    println!("pubkey: {:?}", keypair.pubkey());

    let exit = Arc::new(AtomicBool::new(false));

    let is_mainnet = false;

    let entrypoint_addr = if is_mainnet {
        get_mainnet_entrypoint_socket_addr()
    } else {
        get_devnet_entrypoint_socket_addr()
    };
    let entrypoint = Some(&entrypoint_addr);

    let mut gossip_member_whitelist = if is_mainnet {
        get_mainnet_entrypoint_keys()
    } else {
        get_devnet_entrypoint_keys()
    };

    let gossip_addr_addr = SocketAddr::from(([186, 233, 187, 23], 8002));
    let gossip_addr = Some(&gossip_addr_addr);

    // can use 0 and get from entrypoint
    let shred_version = if is_mainnet { 50093 } else { 45127 };
    let should_check_duplicate_instance = true;
    let socket_addr_space = SocketAddrSpace::Global;

    let feature_set = u32::from_le_bytes(
        solana_sdk::feature_set::ID.as_ref()[..4]
            .try_into()
            .unwrap(),
    );

    let static_feature_set = 3469865029u32;
    let commit = solana_version::compute_commit(Some("d0b1f2c7c0ac90543ed6935f65b7cfc4673f74da"))
        .unwrap_or_default();

    let solana_version = Version::new(1, 18, 11, commit, static_feature_set);

    println!("solana_version: {:?}", solana_version.as_semver_version());

    let _gossip_node = make_beacon_node(
        keypair,
        entrypoint,
        exit.clone(),
        gossip_addr,
        shred_version,
        should_check_duplicate_instance,
        socket_addr_space,
        solana_version,
        None,
    );

    loop {
        if exit.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_secs(30));
        println!("listening: {:?}", std::time::SystemTime::now());
    }
}

fn node_emu_headless() {
    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let api_url = "https://api.devnet.solana.com";

    let beacon_keypair = Keypair::new();
    let beacon_pubkey = beacon_keypair.pubkey();

    println!("beacon_pubkey: {:?}", beacon_pubkey);

    let shred_version = 45127;
    let static_feature_set = 3469865029u32;
    let commit = solana_version::compute_commit(Some("d0b1f2c7c0ac90543ed6935f65b7cfc4673f74da"))
        .unwrap_or_default();
    let solana_version = Version::new(1, 18, 11, commit, static_feature_set);
    let legacy_solana_version = solana_version.to_legacy_version_2();
    let socket_addr_space = SocketAddrSpace::Unspecified;

    let gossip_addr_addr = SocketAddr::from(([186, 233, 187, 23], 8002));
    let gossip_addr = gossip_addr_addr;

    let mut rng = rand::thread_rng();

    let rpc_client = RpcClient::new(api_url.to_string());
    let all_cluster_nodes = rpc_client.get_cluster_nodes();

    let bind_ip_addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let (port, (gossip_socket, ip_echo)) =
        Node::get_gossip_port(&gossip_addr, VALIDATOR_PORT_RANGE, bind_ip_addr);

    let recycler = PacketBatchRecycler::default();

    if let Ok(cluster_nodes) = all_cluster_nodes {
        while true {
            for cluster_node in &cluster_nodes {
                if let Some(cluster_node_gossip) = cluster_node.gossip {
                    /*
                    let crds_node_instance =
                        NodeInstance::new(&mut rng, beacon_pubkey, timestamp());
                    let crds_version = crds_value::Version::new_with_version(
                        beacon_pubkey,
                        legacy_solana_version.clone(),
                    ); */

                    let contact_info = ContactInfo::new_versioned(
                        beacon_pubkey,
                        timestamp(),
                        shred_version,
                        solana_version.clone(),
                    );

                    let self_info = LegacyContactInfo::try_from(&contact_info)
                        .map(CrdsData::LegacyContactInfo)
                        .expect("Operator must spin up node with valid contact-info");
                    let self_info = CrdsValue::new_signed(self_info, &beacon_keypair);

                    /*                    let signed_contact_info =
                                           CrdsValue::new_signed(CrdsData::ContactInfo(contact_info), &beacon_keypair);
                    */

                    let request = solana_gossip::cluster_info::Protocol::PullRequest(
                        CrdsFilter::default(),
                        self_info.clone(),
                    );

                    let mut gossip_requests =
                        Vec::<(SocketAddr, solana_gossip::cluster_info::Protocol)>::new();
                    gossip_requests.push((cluster_node_gossip, request));

                    let packet_batch = PacketBatch::new_unpinned_with_recycler_data_and_dests(
                        &recycler,
                        "run_gossip",
                        &gossip_requests,
                    );

                    let packets = packet_batch.iter().filter_map(|pkt| {
                        let addr = pkt.meta().socket_addr();
                        let data = pkt.data(..)?;
                        socket_addr_space.check(&addr).then_some((data, addr))
                    });

                    let send_payload = &packets.collect::<Vec<_>>();

                    if let Err(e) = batch_send(&gossip_socket, send_payload) {
                        eprintln!("Failed to send gossip packets: {}", e);
                    }
                }
            }
            // thread::sleep(Duration::from_secs(10));
            println!("propagating")
        }
    } else {
        eprintln!(
            "failed to get cluster nodes: {}",
            all_cluster_nodes.err().unwrap()
        );
    }
}

fn main() {
    node_emu()
}
