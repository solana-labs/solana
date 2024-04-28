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
    std::{
        collections::HashSet,
        env,
        net::SocketAddr,
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

fn main() {
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

    let gossip_member_whitelist = if is_mainnet {
        get_mainnet_entrypoint_keys()
    } else {
        get_devnet_entrypoint_keys()
    };

    let gossip_addr_addr = SocketAddr::from(([186, 233, 187, 23], 8002));
    let gossip_addr = Some(&gossip_addr_addr);

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
        Some(gossip_member_whitelist),
    );

    loop {
        if exit.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_secs(30));
        println!("listening: {:?}", std::time::SystemTime::now());
    }
}
