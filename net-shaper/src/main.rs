#![allow(clippy::arithmetic_side_effects)]
use {
    clap::{crate_description, crate_name, crate_version, Arg, ArgMatches, Command},
    rand::{thread_rng, Rng},
    serde::{Deserialize, Serialize},
    std::{fs, io, path::PathBuf},
};

#[derive(Deserialize, Serialize, Debug)]
struct NetworkInterconnect {
    pub a: u8,
    pub b: u8,
    pub config: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct NetworkTopology {
    pub partitions: Vec<u8>,
    pub interconnects: Vec<NetworkInterconnect>,
}

impl Default for NetworkTopology {
    fn default() -> Self {
        Self {
            partitions: vec![100],
            interconnects: vec![],
        }
    }
}

impl NetworkTopology {
    pub fn verify(&self) -> bool {
        let sum: u8 = self.partitions.iter().sum();
        if sum != 100 {
            return false;
        }

        for x in self.interconnects.iter() {
            if x.a as usize > self.partitions.len() || x.b as usize > self.partitions.len() {
                return false;
            }
        }

        true
    }

    pub fn new_from_stdin() -> Self {
        let mut input = String::new();
        println!("Configure partition map (must add up to 100, e.g. [70, 20, 10]):");
        let partitions_str = match io::stdin().read_line(&mut input) {
            Ok(_) => input,
            Err(error) => panic!("error: {error}"),
        };

        let partitions: Vec<u8> = serde_json::from_str(&partitions_str)
            .expect("Failed to parse input. It must be a JSON string");

        let mut interconnects: Vec<NetworkInterconnect> = vec![];

        for i in 0..partitions.len() - 1 {
            for j in i + 1..partitions.len() {
                println!("Configure interconnect ({i} <-> {j}):");
                let mut input = String::new();
                let mut interconnect_config = match io::stdin().read_line(&mut input) {
                    Ok(_) => input,
                    Err(error) => panic!("error: {error}"),
                };

                if interconnect_config.ends_with('\n') {
                    interconnect_config.pop();
                    if interconnect_config.ends_with('\r') {
                        interconnect_config.pop();
                    }
                }

                if !interconnect_config.is_empty() {
                    let interconnect = NetworkInterconnect {
                        a: i as u8,
                        b: j as u8,
                        config: interconnect_config.clone(),
                    };
                    interconnects.push(interconnect);
                    let interconnect = NetworkInterconnect {
                        a: j as u8,
                        b: i as u8,
                        config: interconnect_config,
                    };
                    interconnects.push(interconnect);
                }
            }
        }

        Self {
            partitions,
            interconnects,
        }
    }

    fn new_random(max_partitions: usize, max_packet_drop: u8, max_packet_delay: u32) -> Self {
        let mut rng = thread_rng();
        let num_partitions = rng.gen_range(0..max_partitions + 1);

        if num_partitions == 0 {
            return NetworkTopology::default();
        }

        let mut partitions = vec![];
        let mut used_partition = 0;
        for i in 0..num_partitions {
            let partition = if i == num_partitions - 1 {
                100 - used_partition
            } else {
                rng.gen_range(0..100 - used_partition - num_partitions + i)
            };
            used_partition += partition;
            partitions.push(partition as u8);
        }

        let mut interconnects: Vec<NetworkInterconnect> = vec![];
        for i in 0..partitions.len() - 1 {
            for j in i + 1..partitions.len() {
                let drop_config = if max_packet_drop > 0 {
                    let packet_drop = rng.gen_range(0..max_packet_drop + 1);
                    format!("loss {packet_drop}% 25% ")
                } else {
                    String::default()
                };

                let config = if max_packet_delay > 0 {
                    let packet_delay = rng.gen_range(0..max_packet_delay + 1);
                    format!("{drop_config}delay {packet_delay}ms 10ms")
                } else {
                    drop_config
                };

                let interconnect = NetworkInterconnect {
                    a: i as u8,
                    b: j as u8,
                    config: config.clone(),
                };
                interconnects.push(interconnect);
                let interconnect = NetworkInterconnect {
                    a: j as u8,
                    b: i as u8,
                    config,
                };
                interconnects.push(interconnect);
            }
        }
        Self {
            partitions,
            interconnects,
        }
    }
}

fn run(
    cmd: &str,
    args: &[&str],
    launch_err_msg: &str,
    status_err_msg: &str,
    ignore_err: bool,
) -> bool {
    println!("Running {:?}", std::process::Command::new(cmd).args(args));
    let output = std::process::Command::new(cmd)
        .args(args)
        .output()
        .expect(launch_err_msg);

    if ignore_err {
        return true;
    }

    if !output.status.success() {
        eprintln!(
            "{} command failed with exit code: {}",
            status_err_msg, output.status
        );
        use std::str::from_utf8;
        println!("stdout: {}", from_utf8(&output.stdout).unwrap_or("?"));
        println!("stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));
        false
    } else {
        true
    }
}

fn insert_iptables_rule(tos: u8) -> bool {
    let my_tos = tos.to_string();

    // iptables -t mangle -A PREROUTING -p udp -j TOS --set-tos <my_parition_index>
    run(
        "iptables",
        &[
            "-t",
            "mangle",
            "-A",
            "OUTPUT",
            "-p",
            "udp",
            "-j",
            "TOS",
            "--set-tos",
            my_tos.as_str(),
        ],
        "Failed to add iptables rule",
        "iptables",
        false,
    )
}

fn flush_iptables_rule() {
    run(
        "iptables",
        &["-F", "-t", "mangle"],
        "Failed to flush iptables",
        "iptables flush",
        true,
    );
}

fn setup_ifb(interface: &str) -> bool {
    // modprobe ifb numifbs=1
    run(
        "modprobe",
        &[
            "ifb", "numifbs=1",
        ],
        "Failed to load ifb module",
        "modprobe ifb numifbs=1",
        false
    ) &&
    // ip link set dev ifb0 up
    run(
        "ip",
        &[
            "link", "set", "dev", "ifb0", "up"
        ],
        "Failed to bring ifb0 online",
        "ip link set dev ifb0 up",
        false
    ) &&
    // tc qdisc add dev <if> handle ffff: ingress
    run(
        "tc",
        &[
            "qdisc", "add", "dev", interface, "handle", "ffff:", "ingress"
        ],
        "Failed to setup ingress qdisc",
        "tc qdisc add dev <if> handle ffff: ingress",
        false
    )
    &&
    // tc filter add dev <if> parent ffff: protocol ip u32 match u32 0 0 action mirred egress redirect dev ifb0
    run(
        "tc",
        &[
            "filter", "add", "dev", interface, "parent", "ffff:", "protocol", "ip", "u32", "match", "u32", "0", "0", "action", "mirred", "egress", "redirect", "dev", "ifb0"
        ],
        "Failed to redirect ingress traffc",
        "tc filter add dev <if> parent ffff: protocol ip u32 match u32 0 0 action mirred egress redirect dev ifb0",
        false
    )
}

fn delete_ifb(interface: &str) -> bool {
    run(
        "tc",
        &[
            "qdisc", "delete", "dev", interface, "handle", "ffff:", "ingress",
        ],
        "Failed to setup ingress qdisc",
        "tc qdisc delete dev <if> handle ffff: ingress",
        true,
    ) && run(
        "modprobe",
        &["ifb", "--remove"],
        "Failed to delete ifb module",
        "modprobe ifb --remove",
        true,
    )
}

fn insert_tc_ifb_root(num_bands: &str) -> bool {
    // tc qdisc add dev ifb0 root handle 1: prio bands <num_bands>
    run(
        "tc",
        &[
            "qdisc", "add", "dev", "ifb0", "root", "handle", "1:", "prio", "bands", num_bands,
        ],
        "Failed to add root ifb qdisc",
        "tc qdisc add dev ifb0 root handle 1: prio bands <num_bands>",
        false,
    )
}

fn insert_tc_ifb_netem(class: &str, handle: &str, filter: &str) -> bool {
    let mut filters: Vec<&str> = filter.split(' ').collect();
    let mut args = vec![
        "qdisc", "add", "dev", "ifb0", "parent", class, "handle", handle, "netem",
    ];
    args.append(&mut filters);
    // tc qdisc add dev ifb0 parent <class> handle <handle> netem <filters>
    run("tc", &args, "Failed to add tc child", "tc add child", false)
}

fn insert_tos_ifb_filter(class: &str, tos: &str) -> bool {
    // tc filter add dev ifb0 protocol ip parent 1: prio 1 u32 match ip tos <tos> 0xff flowid <class>
    run(
        "tc",
        &[
            "filter", "add", "dev", "ifb0", "protocol", "ip", "parent", "1:", "prio", "1",
            "u32", "match", "ip", "tos", tos, "0xff", "flowid", class,
        ],
        "Failed to add tos filter",
        "tc filter add dev ifb0 protocol ip parent 1: prio 1 u32 match ip tos <tos> 0xff flowid <class>",
        false,
    )
}

fn insert_default_ifb_filter(class: &str) -> bool {
    // tc filter add dev ifb0 parent 1: protocol all prio 2 u32 match u32 0 0 flowid 1:<class>
    run(
        "tc",
        &[
            "filter", "add", "dev", "ifb0", "parent", "1:", "protocol", "all", "prio", "2", "u32",
            "match", "u32", "0", "0", "flowid", class,
        ],
        "Failed to add catch-all filter",
        "tc filter add dev ifb0 parent 1: protocol all prio 2 u32 match u32 0 0 flowid 1:<class>",
        false,
    )
}

fn delete_all_filters(interface: &str) {
    // tc filter delete dev <if>
    run(
        "tc",
        &["filter", "delete", "dev", interface],
        "Failed to delete all filters",
        "tc delete all filters",
        true,
    );
}

fn identify_my_partition(partitions: &[u8], index: u64, size: u64) -> usize {
    let mut my_partition = 0;
    let mut watermark = 0;
    for (i, p) in partitions.iter().enumerate() {
        watermark += *p;
        if u64::from(watermark) >= index * 100 / size {
            my_partition = i;
            break;
        }
    }

    my_partition
}

fn partition_id_to_tos(partition: usize) -> u8 {
    if partition < 4 {
        2u8.pow(partition as u32 + 1)
    } else {
        0
    }
}

fn shape_network(matches: &ArgMatches) {
    let config_path = PathBuf::from(matches.value_of_t_or_exit::<String>("file"));
    let config = fs::read_to_string(config_path).expect("Unable to read config file");
    let topology: NetworkTopology =
        serde_json::from_str(&config).expect("Failed to parse log as JSON");
    let interface: String = matches.value_of_t_or_exit("iface");
    let network_size: u64 = matches.value_of_t_or_exit("size");
    let my_index: u64 = matches.value_of_t_or_exit("position");
    if !shape_network_steps(&topology, &interface, network_size, my_index) {
        delete_ifb(interface.as_str());
        flush_iptables_rule();
    }
}

fn shape_network_steps(
    topology: &NetworkTopology,
    interface: &str,
    network_size: u64,
    my_index: u64,
) -> bool {
    // Integrity checks
    assert!(topology.verify(), "Failed to verify the configuration file");
    assert!(my_index < network_size);

    // Figure out partition we belong in
    let my_partition = identify_my_partition(&topology.partitions, my_index + 1, network_size);

    // Clear any lingering state
    println!(
        "my_index: {}, network_size: {}, partitions: {:?}",
        my_index, network_size, topology.partitions
    );
    println!("My partition is {my_partition}");

    cleanup_network(interface);

    // Mark egress packets with our partition id
    if !insert_iptables_rule(partition_id_to_tos(my_partition)) {
        return false;
    }

    let num_bands = topology.partitions.len() + 1;
    let default_filter_class = format!("1:{num_bands}");
    if !topology.interconnects.is_empty() {
        let num_bands_str = num_bands.to_string();
        // Redirect ingress traffic to the virtual interface ifb0 so we can
        // apply egress rules
        if !setup_ifb(interface)
            // Setup root qdisc on ifb0
            || !insert_tc_ifb_root(num_bands_str.as_str())
            // Catch all so regular traffic/traffic within the same partition
            // is not filtered out
            || !insert_default_ifb_filter(default_filter_class.as_str())
        {
            return false;
        }
    }

    println!("Setting up interconnects");
    for i in &topology.interconnects {
        if i.b as usize == my_partition {
            println!("interconnects: {i:#?}");
            let tos = partition_id_to_tos(i.a as usize);
            if tos == 0 {
                println!("Incorrect value of TOS/Partition in config {}", i.a);
                return false;
            }
            let tos_string = tos.to_string();
            // First valid class is 1:1
            let class = format!("1:{}", i.a + 1);
            if !insert_tc_ifb_netem(class.as_str(), tos_string.as_str(), i.config.as_str()) {
                return false;
            }

            if !insert_tos_ifb_filter(class.as_str(), tos_string.as_str()) {
                return false;
            }
        }
    }

    true
}

fn parse_interface(interfaces: &str) -> &str {
    for line in interfaces.lines() {
        if line != "ifb0" {
            return line;
        }
    }

    panic!("No valid interfaces");
}

fn cleanup_network(interface: &str) {
    delete_all_filters("ifb0");
    delete_ifb(interface);
    flush_iptables_rule();
}

fn configure(matches: &ArgMatches) {
    let config = if !matches.is_present("random") {
        NetworkTopology::new_from_stdin()
    } else {
        let max_partitions: usize = matches.value_of_t("max-partitions").unwrap_or(4);
        let max_drop: u8 = matches.value_of_t("max-drop").unwrap_or(100);
        let max_delay: u32 = matches.value_of_t("max-delay").unwrap_or(50);
        NetworkTopology::new_random(max_partitions, max_drop, max_delay)
    };

    assert!(config.verify(), "Failed to verify the configuration");

    let topology = serde_json::to_string(&config).expect("Failed to write as JSON");

    println!("{topology}");
}

fn main() {
    solana_logger::setup();

    let matches = Command::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .subcommand(
            Command::new("shape")
                .about("Shape the network using config file")
                .arg(
                    Arg::new("file")
                        .short('f')
                        .long("file")
                        .value_name("config file")
                        .takes_value(true)
                        .required(true)
                        .help("Location of the network config file"),
                )
                .arg(
                    Arg::new("size")
                        .short('s')
                        .long("size")
                        .value_name("network size")
                        .takes_value(true)
                        .required(true)
                        .help("Number of nodes in the network"),
                )
                .arg(
                    Arg::new("iface")
                        .short('i')
                        .long("iface")
                        .value_name("network interface name")
                        .takes_value(true)
                        .required(true)
                        .help("Name of network interface"),
                )
                .arg(
                    Arg::new("position")
                        .short('p')
                        .long("position")
                        .value_name("position of node")
                        .takes_value(true)
                        .required(true)
                        .help("Position of current node in the network"),
                ),
        )
        .subcommand(
            Command::new("cleanup")
                .about("Remove the network filters using config file")
                .arg(
                    Arg::new("file")
                        .short('f')
                        .long("file")
                        .value_name("config file")
                        .takes_value(true)
                        .required(true)
                        .help("Location of the network config file"),
                )
                .arg(
                    Arg::new("size")
                        .short('s')
                        .long("size")
                        .value_name("network size")
                        .takes_value(true)
                        .required(true)
                        .help("Number of nodes in the network"),
                )
                .arg(
                    Arg::new("iface")
                        .short('i')
                        .long("iface")
                        .value_name("network interface name")
                        .takes_value(true)
                        .required(true)
                        .help("Name of network interface"),
                )
                .arg(
                    Arg::new("position")
                        .short('p')
                        .long("position")
                        .value_name("position of node")
                        .takes_value(true)
                        .required(true)
                        .help("Position of current node in the network"),
                ),
        )
        .subcommand(
            Command::new("configure")
                .about("Generate a config file")
                .arg(
                    Arg::new("random")
                        .short('r')
                        .long("random")
                        .required(false)
                        .help("Generate a random config file"),
                )
                .arg(
                    Arg::new("max-partitions")
                        .short('p')
                        .long("max-partitions")
                        .value_name("count")
                        .takes_value(true)
                        .required(false)
                        .help("Maximum number of partitions. Used only with random configuration generation"),
                )
                .arg(
                    Arg::new("max-drop")
                        .short('d')
                        .long("max-drop")
                        .value_name("percentage")
                        .takes_value(true)
                        .required(false)
                        .help("Maximum amount of packet drop. Used only with random configuration generation"),
                )
                .arg(
                    Arg::new("max-delay")
                        .short('y')
                        .long("max-delay")
                        .value_name("ms")
                        .takes_value(true)
                        .required(false)
                        .help("Maximum amount of packet delay. Used only with random configuration generation"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("shape", args_matches)) => shape_network(args_matches),
        Some(("cleanup", args_matches)) => {
            let interfaces: String = args_matches.value_of_t_or_exit("iface");
            let iface = parse_interface(&interfaces);
            cleanup_network(iface)
        }
        Some(("configure", args_matches)) => configure(args_matches),
        _ => {}
    };
}
