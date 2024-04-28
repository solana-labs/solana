use {
    solana_gossip::{
        cluster_info::ClusterInfo,
        contact_info::ContactInfo,
        crds_gossip::CrdsGossip,
        crds_value::{CrdsData, CrdsValue},
        gossip_service::{
            discover, discover_cluster, make_beacon_node, make_gossip_node, GossipService,
        },
    },
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_version::Version,
    std::env,
    std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

fn main() {
    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let keypair = Keypair::new();
    let cloned_keypair = keypair.insecure_clone();

    println!("pubkey: {:?}", keypair.pubkey());

    let exit = Arc::new(AtomicBool::new(false));

    let entrypoint_addr = SocketAddr::from(([35, 197, 53, 105], 8001));
    let entrypoint = Some(&entrypoint_addr);
    let gossip_addr_addr = SocketAddr::from(([186, 233, 187, 23], 8002));
    let gossip_addr = Some(&gossip_addr_addr);

    let shred_version = 45127;
    let should_check_duplicate_instance = true;
    let socket_addr_space = SocketAddrSpace::Global;

    let feature_set = u32::from_le_bytes(
        solana_sdk::feature_set::ID.as_ref()[..4]
            .try_into()
            .unwrap(),
    );

    let static_feature_set = 3746964731u32;
    let commit = solana_version::compute_commit(Some("d0b1f2c7c0ac90543ed6935f65b7cfc4673f74da"))
        .unwrap_or_default();

    let solana_version = Version::new(1, 33, 7, commit, static_feature_set);

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
    );

    loop {
        if exit.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_secs(30));
        println!("listening: {:?}", std::time::SystemTime::now());
    }
}
