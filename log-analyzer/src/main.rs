extern crate byte_unit;

use byte_unit::Byte;
use clap::{
    crate_description, crate_name, crate_version, value_t_or_exit, App, Arg, ArgMatches, SubCommand,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::ops::Sub;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Debug)]
struct IPAddrMapping {
    private: String,
    public: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct LogLine {
    a: String,
    b: String,
    a_to_b: String,
    b_to_a: String,
}

impl Default for LogLine {
    fn default() -> Self {
        Self {
            a: String::default(),
            b: String::default(),
            a_to_b: "0B".to_string(),
            b_to_a: "0B".to_string(),
        }
    }
}

impl LogLine {
    fn output(a: &str, b: &str, v1: u128, v2: u128) -> String {
        format!(
            "Lost {}%, {}, ({} - {}), sender {}, receiver {}",
            ((v1 - v2) * 100 / v1),
            Byte::from_bytes(v1 - v2)
                .get_appropriate_unit(true)
                .to_string(),
            Byte::from_bytes(v1).get_appropriate_unit(true).to_string(),
            Byte::from_bytes(v2).get_appropriate_unit(true).to_string(),
            a,
            b
        )
    }
}

impl Sub for &LogLine {
    type Output = String;

    fn sub(self, rhs: Self) -> Self::Output {
        let a_to_b = Byte::from_str(&self.a_to_b)
            .expect("Failed to read a_to_b bytes")
            .get_bytes();
        let b_to_a = Byte::from_str(&self.b_to_a)
            .expect("Failed to read b_to_a bytes")
            .get_bytes();
        let rhs_a_to_b = Byte::from_str(&rhs.a_to_b)
            .expect("Failed to read a_to_b bytes")
            .get_bytes();
        let rhs_b_to_a = Byte::from_str(&rhs.b_to_a)
            .expect("Failed to read b_to_a bytes")
            .get_bytes();
        let mut out1 = if a_to_b > rhs_b_to_a {
            LogLine::output(&self.a, &self.b, a_to_b, rhs_b_to_a)
        } else if a_to_b < rhs_b_to_a {
            LogLine::output(&self.b, &self.a, rhs_b_to_a, a_to_b)
        } else {
            String::default()
        };
        let out2 = if rhs_a_to_b > b_to_a {
            LogLine::output(&self.a, &self.b, rhs_a_to_b, b_to_a)
        } else if rhs_a_to_b < b_to_a {
            LogLine::output(&self.b, &self.a, b_to_a, rhs_a_to_b)
        } else {
            String::default()
        };
        if !out1.is_empty() && !out2.is_empty() {
            out1.push('\n');
        }
        out1.push_str(&out2);
        out1
    }
}

fn map_ip_address(mappings: &[IPAddrMapping], target: String) -> String {
    for mapping in mappings {
        if target.contains(&mapping.private) {
            return target.replace(&mapping.private, mapping.public.as_str());
        }
    }
    target.to_string()
}

fn process_iftop_logs(matches: &ArgMatches) {
    let mut map_list: Vec<IPAddrMapping> = vec![];
    if let ("map-IP", Some(args_matches)) = matches.subcommand() {
        let mut list = args_matches
            .value_of("list")
            .expect("Missing list of IP address mappings")
            .to_string();
        list.insert(0, '[');
        let terminate_at = list
            .rfind('}')
            .expect("Didn't find a terminating '}' in IP list")
            + 1;
        let _ = list.split_off(terminate_at);
        list.push(']');
        map_list = serde_json::from_str(&list).expect("Failed to parse IP address mapping list");
    };

    let log_path = PathBuf::from(value_t_or_exit!(matches, "file", String));
    let mut log = fs::read_to_string(&log_path).expect("Unable to read log file");
    log.insert(0, '[');
    let terminate_at = log.rfind('}').expect("Didn't find a terminating '}'") + 1;
    let _ = log.split_off(terminate_at);
    log.push(']');
    let json_log: Vec<LogLine> = serde_json::from_str(&log).expect("Failed to parse log as JSON");

    let mut unique_latest_logs = HashMap::new();

    json_log.into_iter().rev().for_each(|l| {
        if !l.a.is_empty() && !l.b.is_empty() && !l.a_to_b.is_empty() && !l.b_to_a.is_empty() {
            let key = (l.a.clone(), l.b.clone());
            unique_latest_logs.entry(key).or_insert(l);
        }
    });
    let output: Vec<LogLine> = unique_latest_logs
        .into_iter()
        .map(|(_, l)| {
            if map_list.is_empty() {
                l
            } else {
                LogLine {
                    a: map_ip_address(&map_list, l.a),
                    b: map_ip_address(&map_list, l.b),
                    a_to_b: l.a_to_b,
                    b_to_a: l.b_to_a,
                }
            }
        })
        .collect();

    println!("{}", serde_json::to_string(&output).unwrap());
}

fn analyze_logs(matches: &ArgMatches) {
    let dir_path = PathBuf::from(value_t_or_exit!(matches, "folder", String));
    if !dir_path.is_dir() {
        panic!("Need a folder that contains all log files");
    }
    let list_all_diffs = matches.is_present("all");
    let files = fs::read_dir(dir_path).expect("Failed to read log folder");
    let logs: Vec<_> = files
        .flat_map(|f| {
            if let Ok(f) = f {
                let log_str = fs::read_to_string(&f.path()).expect("Unable to read log file");
                let log: Vec<LogLine> =
                    serde_json::from_str(log_str.as_str()).expect("Failed to deserialize log");
                log
            } else {
                vec![]
            }
        })
        .collect();
    let mut logs_hash = HashMap::new();
    logs.iter().for_each(|l| {
        let key = (l.a.clone(), l.b.clone());
        logs_hash.entry(key).or_insert(l);
    });

    logs.iter().for_each(|l| {
        let diff = logs_hash
            .remove(&(l.a.clone(), l.b.clone()))
            .map(|v1| {
                logs_hash.remove(&(l.b.clone(), l.a.clone())).map_or(
                    if list_all_diffs {
                        v1 - &LogLine::default()
                    } else {
                        String::default()
                    },
                    |v2| v1 - v2,
                )
            })
            .unwrap_or_default();
        if !diff.is_empty() {
            println!("{}", diff);
        }
    });
}

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .subcommand(
            SubCommand::with_name("iftop")
                .about("Process iftop log file")
                .arg(
                    Arg::with_name("file")
                        .short("f")
                        .long("file")
                        .value_name("iftop log file")
                        .takes_value(true)
                        .help("Location of the log file generated by iftop"),
                )
                .subcommand(
                    SubCommand::with_name("map-IP")
                        .about("Map private IP to public IP Address")
                        .arg(
                            Arg::with_name("list")
                                .short("l")
                                .long("list")
                                .value_name("JSON string")
                                .takes_value(true)
                                .required(true)
                                .help("JSON string with a list of mapping"),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("analyze")
                .about("Compare processed network log files")
                .arg(
                    Arg::with_name("folder")
                        .short("f")
                        .long("folder")
                        .value_name("DIR")
                        .takes_value(true)
                        .help("Location of processed log files"),
                )
                .arg(
                    Arg::with_name("all")
                        .short("a")
                        .long("all")
                        .takes_value(false)
                        .help("List all differences"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("iftop", Some(args_matches)) => process_iftop_logs(args_matches),
        ("analyze", Some(args_matches)) => analyze_logs(args_matches),
        _ => {}
    };
}
