use clap::{
    crate_description, crate_name, crate_version, value_t_or_exit, App, Arg, ArgMatches, SubCommand,
};

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

fn insert_tc_root(interface: &str, num_bands: &str) -> bool {
    // tc qdisc add dev <if> root handle 1: prio
    // tc qdisc add dev <if> root handle 1: prio bands <num_bands>
    run(
        "tc",
        &[
            "qdisc", "add", "dev", interface, "root", "handle", "1:", "prio", "bands", num_bands,
        ],
        "Failed to add root qdisc",
        "tc add root qdisc",
        false,
    )
}

fn delete_tc_root(interface: &str) {
    // tc qdisc delete dev <if> root handle 1: prio
    run(
        "tc",
        &[
            "qdisc", "delete", "dev", interface, "root", "handle", "1:", "prio",
        ],
        "Failed to delete root qdisc",
        "tc qdisc delete root",
        true,
    );
}

fn insert_tc_netem(interface: &str, class: &str, handle: &str, filter: &str) -> bool {
    let mut filters: Vec<&str> = filter.split(' ').collect();
    let mut args = vec![
        "qdisc", "add", "dev", interface, "parent", class, "handle", handle, "netem",
    ];
    args.append(&mut filters);
    // tc qdisc add dev <if> parent 1:<i.a> handle <i.a>: netem <filters>
    run("tc", &args, "Failed to add tc child", "tc add child", false)
}

fn delete_tc_netem(interface: &str, class: &str, handle: &str, filter: &str) {
    let mut filters: Vec<&str> = filter.split(' ').collect();
    let mut args = vec![
        "qdisc", "delete", "dev", interface, "parent", class, "handle", handle, "netem",
    ];
    args.append(&mut filters);
    // tc qdisc delete dev <if> parent 1:<i.a> handle <i.a>: netem <filters>
    run(
        "tc",
        &args,
        "Failed to delete child qdisc",
        "tc delete child qdisc",
        true,
    );
}

fn insert_tos_filter(interface: &str, class: &str, tos: &str) -> bool {
    // tc filter add dev <if> protocol ip parent 1: prio 1 u32 match ip tos <i.a> 0xff flowid 1:<i.a>
    run(
        "tc",
        &[
            "filter", "add", "dev", interface, "protocol", "ip", "parent", "1:", "prio", "1",
            "u32", "match", "ip", "tos", tos, "0xff", "flowid", class,
        ],
        "Failed to add tos filter",
        "tc add filter",
        false,
    )
}

fn delete_tos_filter(interface: &str, class: &str, tos: &str) {
    // tc filter delete dev <if> protocol ip parent 1: prio 10 u32 match ip tos <i.a> 0xff flowid 1:<i.a>
    run(
        "tc",
        &[
            "filter", "delete", "dev", interface, "protocol", "ip", "parent", "1:", "prio", "1",
            "u32", "match", "ip", "tos", tos, "0xff", "flowid", class,
        ],
        "Failed to delete tos filter",
        "tc delete filter",
        true,
    );
}

fn insert_default_filter(interface: &str, class: &str) -> bool {
    // tc filter add dev <if> protocol ip parent 1: prio 2 u32 match ip src 0/0 flowid 1:<class>
    run(
        "tc",
        &[
            "filter", "add", "dev", interface, "protocol", "ip", "parent", "1:", "prio", "2",
            "u32", "match", "ip", "tos", "0", "0xff", "flowid", class,
        ],
        "Failed to add default filter",
        "tc add default filter",
        false,
    )
}

fn delete_default_filter(interface: &str, class: &str) {
    // tc filter delete dev <if> protocol ip parent 1: prio 2 flowid 1:<class>
    run(
        "tc",
        &[
            "filter", "delete", "dev", interface, "protocol", "ip", "parent", "1:", "prio", "2",
            "flowid", class,
        ],
        "Failed to delete default filter",
        "tc delete default filter",
        true,
    );
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
    let config_path = PathBuf::from(value_t_or_exit!(matches, "file", String));
    let config = fs::read_to_string(&config_path).expect("Unable to read config file");
    let topology: NetworkTopology =
        serde_json::from_str(&config).expect("Failed to parse log as JSON");

    if !topology.verify() {
        panic!("Failed to verify the configuration file");
    }

    let network_size = value_t_or_exit!(matches, "size", u64);
    let my_index = value_t_or_exit!(matches, "position", u64);
    let interface = value_t_or_exit!(matches, "iface", String);

    assert!(my_index < network_size);

    let my_partition = identify_my_partition(&topology.partitions, my_index + 1, network_size);
    println!("My partition is {}", my_partition);

    flush_iptables_rule();

    if !insert_iptables_rule(partition_id_to_tos(my_partition)) {
        return;
    }

    delete_tc_root(interface.as_str());
    let num_bands = topology.partitions.len() + 1;
    let default_filter_class = format!("1:{}", num_bands);
    if !topology.interconnects.is_empty() {
        let num_bands_str = num_bands.to_string();
        if !insert_tc_root(interface.as_str(), num_bands_str.as_str())
            || !insert_default_filter(interface.as_str(), default_filter_class.as_str())
        {
            delete_tc_root(interface.as_str());
            flush_iptables_rule();
            return;
        }
    }

    topology.interconnects.iter().for_each(|i| {
        if i.b as usize == my_partition {
            let tos = partition_id_to_tos(i.a as usize);
            if tos == 0 {
                println!("Incorrect value of TOS/Partition in config {}", i.a);
                delete_default_filter(interface.as_str(), default_filter_class.as_str());
                delete_tc_root(interface.as_str());
                return;
            }
            let tos_string = tos.to_string();
            // First valid class is 1:1
            let class = format!("1:{}", i.a + 1);
            if !insert_tc_netem(
                interface.as_str(),
                class.as_str(),
                tos_string.as_str(),
                i.config.as_str(),
            ) {
                delete_default_filter(interface.as_str(), default_filter_class.as_str());
                delete_tc_root(interface.as_str());
                return;
            }

            if !insert_tos_filter(interface.as_str(), class.as_str(), tos_string.as_str()) {
                delete_tc_netem(
                    interface.as_str(),
                    class.as_str(),
                    tos_string.as_str(),
                    i.config.as_str(),
                );
                delete_default_filter(interface.as_str(), default_filter_class.as_str());
                delete_tc_root(interface.as_str());
                return;
            }
        }
    })
}

fn cleanup_network(matches: &ArgMatches) {
    let config_path = PathBuf::from(value_t_or_exit!(matches, "file", String));
    let config = fs::read_to_string(&config_path).expect("Unable to read config file");
    let topology: NetworkTopology =
        serde_json::from_str(&config).expect("Failed to parse log as JSON");

    if !topology.verify() {
        panic!("Failed to verify the configuration file");
    }

    let network_size = value_t_or_exit!(matches, "size", u64);
    let my_index = value_t_or_exit!(matches, "position", u64);
    let interface = value_t_or_exit!(matches, "iface", String);

    assert!(my_index < network_size);

    let my_partition = identify_my_partition(&topology.partitions, my_index, network_size);
    println!("My partition is {}", my_partition);

    topology.interconnects.iter().for_each(|i| {
        if i.b as usize == my_partition {
            let handle = (i.a + 1).to_string();
            // First valid class is 1:1
            let class = format!("1:{}", i.a + 1);
            let tos_string = i.a.to_string();
            delete_tos_filter(interface.as_str(), class.as_str(), tos_string.as_str());
            delete_tc_netem(
                interface.as_str(),
                class.as_str(),
                handle.as_str(),
                i.config.as_str(),
            );
        }
    });
    let num_bands = topology.partitions.len() + 1;
    let default_filter_class = format!("1:{}", num_bands);
    delete_default_filter(interface.as_str(), default_filter_class.as_str());
    delete_tc_root(interface.as_str());
    flush_iptables_rule();
}

fn force_cleanup_network(matches: &ArgMatches) {
    let interface = value_t_or_exit!(matches, "iface", String);
    delete_all_filters(interface.as_str());
    delete_tc_root(interface.as_str());
    flush_iptables_rule();
}

fn configure(_matches: &ArgMatches) {
    let config = NetworkTopology::default();
    let topology = serde_json::to_string(&config).expect("Failed to write as JSON");

    println!("{}", topology);
}

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .subcommand(
            SubCommand::with_name("shape")
                .about("Shape the network using config file")
                .arg(
                    Arg::with_name("file")
                        .short("f")
                        .long("file")
                        .value_name("config file")
                        .takes_value(true)
                        .required(true)
                        .help("Location of the network config file"),
                )
                .arg(
                    Arg::with_name("size")
                        .short("s")
                        .long("size")
                        .value_name("network size")
                        .takes_value(true)
                        .required(true)
                        .help("Number of nodes in the network"),
                )
                .arg(
                    Arg::with_name("iface")
                        .short("i")
                        .long("iface")
                        .value_name("network interface name")
                        .takes_value(true)
                        .required(true)
                        .help("Name of network interface"),
                )
                .arg(
                    Arg::with_name("position")
                        .short("p")
                        .long("position")
                        .value_name("position of node")
                        .takes_value(true)
                        .required(true)
                        .help("Position of current node in the network"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cleanup")
                .about("Remove the network filters using config file")
                .arg(
                    Arg::with_name("file")
                        .short("f")
                        .long("file")
                        .value_name("config file")
                        .takes_value(true)
                        .required(true)
                        .help("Location of the network config file"),
                )
                .arg(
                    Arg::with_name("size")
                        .short("s")
                        .long("size")
                        .value_name("network size")
                        .takes_value(true)
                        .required(true)
                        .help("Number of nodes in the network"),
                )
                .arg(
                    Arg::with_name("iface")
                        .short("i")
                        .long("iface")
                        .value_name("network interface name")
                        .takes_value(true)
                        .required(true)
                        .help("Name of network interface"),
                )
                .arg(
                    Arg::with_name("position")
                        .short("p")
                        .long("position")
                        .value_name("position of node")
                        .takes_value(true)
                        .required(true)
                        .help("Position of current node in the network"),
                ),
        )
        .subcommand(
            SubCommand::with_name("force_cleanup")
                .about("Remove the network filters")
                .arg(
                    Arg::with_name("iface")
                        .short("i")
                        .long("iface")
                        .value_name("network interface name")
                        .takes_value(true)
                        .required(true)
                        .help("Name of network interface"),
                ),
        )
        .subcommand(SubCommand::with_name("configure").about("Generate a config file"))
        .get_matches();

    match matches.subcommand() {
        ("shape", Some(args_matches)) => shape_network(args_matches),
        ("cleanup", Some(args_matches)) => cleanup_network(args_matches),
        ("force_cleanup", Some(args_matches)) => force_cleanup_network(args_matches),
        ("configure", Some(args_matches)) => configure(args_matches),
        _ => {}
    };
}
