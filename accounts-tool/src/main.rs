#![allow(clippy::integer_arithmetic)]
use {
    clap::{crate_description, crate_name, value_t_or_exit, App, AppSettings, Arg, SubCommand},
    log::*,
    solana_measure::measure::Measure,
    solana_runtime::append_vec::AppendVec,
    solana_sdk::pubkey::Pubkey,
    std::{cmp::Reverse, collections::BinaryHeap, process::exit},
};

#[derive(Debug, Eq)]
struct TopAccountsStatsEntry<V: std::cmp::Ord> {
    key: Pubkey,
    value: V,
}

impl<V: std::cmp::Ord> Ord for TopAccountsStatsEntry<V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<V: std::cmp::Ord> PartialOrd for TopAccountsStatsEntry<V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V: std::cmp::Ord> PartialEq for TopAccountsStatsEntry<V> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[derive(PartialEq)]
enum TopAccountsRankingField {
    AccountDataSize,
}

impl TopAccountsRankingField {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "data_size" => Some(Self::AccountDataSize),
            _ => None,
        }
    }
}

#[allow(clippy::cognitive_complexity)]
fn main() {
    solana_logger::setup_with_default("solana=info");

    let mut measure_total_execution_time = Measure::start("accounts tool");

    let default_top_accounts_file_count_limit = &std::usize::MAX.to_string();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::InferSubcommands)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
        .arg(
            Arg::with_name("accounts_path")
                .short("p")
                .long("path")
                .takes_value(true)
                .value_name("DIR")
                .help("Path to the accounts db.")
        )
        .subcommand(
            SubCommand::with_name("top-accounts")
            .about("Print the top N accounts of the specified field.")
            .arg(
                Arg::with_name("field")
                    .long("field")
                    .takes_value(true)
                    .value_name("FIELD")
                    .possible_values(&["data_size"])
                    .default_value("data_size")
                    .help("Determine which stats to print. \
                           Possible values are: \
                           'data_size': print the top N accounts with the largest account data size.")
            )
            .arg(
                Arg::with_name("top")
                    .long("top")
                    .takes_value(true)
                    .value_name("N")
                    .default_value("30")
                    .help("Collect the top N entries of the specified --field")
            )
            .arg(
                Arg::with_name("limit")
                    .long("limit")
                    .takes_value(true)
                    .value_name("LIMIT")
                    .default_value(default_top_accounts_file_count_limit)
                    .help("Collect stats from up to LIMIT accounts db files.")
            )
        )
        .get_matches();

    match matches.subcommand() {
        ("top-accounts", Some(arg_matches)) => {
            let accounts_db_path = value_t_or_exit!(arg_matches, "accounts_path", String);
            let heap_size = value_t_or_exit!(arg_matches, "top", usize);
            let limit = value_t_or_exit!(arg_matches, "limit", usize);
            let field_str = value_t_or_exit!(arg_matches, "field", String);
            let field = TopAccountsRankingField::from_str(&field_str).unwrap();

            let mut min_heap = BinaryHeap::new();
            let mut file_count = 0;

            let mut accounts_db_paths = Vec::new();
            accounts_db_paths.push(accounts_db_path);

            while !accounts_db_paths.is_empty() {
                let path = accounts_db_paths.pop();
                let append_vec_paths = std::fs::read_dir(path.unwrap()).unwrap();
                debug!("paths = {:?}", append_vec_paths);
                for path in append_vec_paths {
                    debug!("Collecting stats from {:?}", path);
                    let av_path = path.expect("success").path();
                    let av_len = std::fs::metadata(&av_path).unwrap().len() as usize;
                    if let Ok(mut append_vec) =
                        AppendVec::new_from_file_unchecked(av_path.clone(), av_len)
                    {
                        append_vec.set_no_remove_on_drop();

                        // read append-vec
                        let mut offset = 0;
                        while let Some((account, next_offset)) = append_vec.get_account(offset) {
                            offset = next_offset;
                            // data_size
                            min_heap.push(Reverse(TopAccountsStatsEntry {
                                key: *account.pubkey(),
                                value: match field {
                                    TopAccountsRankingField::AccountDataSize => account.data.len(),
                                },
                            }));
                            if min_heap.len() > heap_size {
                                min_heap.pop();
                            }
                        }
                        file_count += 1;
                        if file_count >= limit {
                            break;
                        }
                    } else if av_path.is_dir() {
                        if let Some(path_str) = av_path.to_str() {
                            info!("discovering subdirectory {:?} as well", av_path);
                            accounts_db_paths.push(path_str.to_string());
                        }
                    }
                }
            }
            println!("Collected top {:?} samples", min_heap.len());
            while !min_heap.is_empty() {
                if let Some(Reverse(entry)) = min_heap.pop() {
                    println!("account: {:?}, {}: {:?}", entry.key, field_str, entry.value);
                }
            }
        }
        ("", _) => {
            eprintln!("{}", matches.usage());
            measure_total_execution_time.stop();
            info!("{}", measure_total_execution_time);
            exit(1);
        }
        _ => unreachable!(),
    };
}
