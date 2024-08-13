use {
    ahash::{HashMap, RandomState},
    bytemuck::Zeroable as _,
    clap::{
        crate_description, crate_name, value_t_or_exit, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    memmap2::Mmap,
    rayon::prelude::*,
    solana_accounts_db::{
        accounts_hash::AccountHash, parse_cache_hash_data_filename,
        pubkey_bins::PubkeyBinCalculator24, CacheHashDataFileEntry, CacheHashDataFileHeader,
        ParsedCacheHashDataFilename,
    },
    solana_program::pubkey::Pubkey,
    std::{
        cmp::{self, Ordering},
        fs::{self, File, Metadata},
        io::{self, BufReader, Read},
        iter,
        mem::size_of,
        num::Saturating,
        path::{Path, PathBuf},
        str,
        sync::RwLock,
        time::Instant,
    },
};

const CMD_INSPECT: &str = "inspect";
const CMD_DIFF: &str = "diff";
const CMD_DIFF_FILES: &str = "files";
const CMD_DIFF_DIRS: &str = "directories";
const CMD_DIFF_STATE: &str = "state";

const DEFAULT_BINS: &str = "8192";

fn main() {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .global_setting(AppSettings::ArgRequiredElseHelp)
        .global_setting(AppSettings::ColoredHelp)
        .global_setting(AppSettings::InferSubcommands)
        .global_setting(AppSettings::UnifiedHelpMessage)
        .global_setting(AppSettings::VersionlessSubcommands)
        .subcommand(
            SubCommand::with_name(CMD_INSPECT)
                .about(
                    "Inspect an accounts hash cache file and display \
                     each account's address, hash, and balance",
                )
                .arg(
                    Arg::with_name("force")
                        .long("force")
                        .takes_value(false)
                        .help("Continue even if sanity checks fail"),
                )
                .arg(
                    Arg::with_name("path")
                        .index(1)
                        .takes_value(true)
                        .value_name("PATH")
                        .help("Accounts hash cache file to inspect"),
                ),
        )
        .subcommand(
            SubCommand::with_name(CMD_DIFF)
                .about("Compares cache files")
                .subcommand(
                    SubCommand::with_name(CMD_DIFF_FILES)
                        .about("Diff two accounts hash cache files")
                        .arg(
                            Arg::with_name("path1")
                                .index(1)
                                .takes_value(true)
                                .value_name("PATH1")
                                .help("Accounts hash cache file 1 to diff"),
                        )
                        .arg(
                            Arg::with_name("path2")
                                .index(2)
                                .takes_value(true)
                                .value_name("PATH2")
                                .help("Accounts hash cache file 2 to diff"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name(CMD_DIFF_DIRS)
                        .about("Diff two accounts hash cache directories")
                        .arg(
                            Arg::with_name("path1")
                                .index(1)
                                .takes_value(true)
                                .value_name("PATH1")
                                .help("Accounts hash cache directory 1 to diff"),
                        )
                        .arg(
                            Arg::with_name("path2")
                                .index(2)
                                .takes_value(true)
                                .value_name("PATH2")
                                .help("Accounts hash cache directory 2 to diff"),
                        )
                        .arg(
                            Arg::with_name("then_diff_files")
                                .long("then-diff-files")
                                .takes_value(false)
                                .help("After diff-ing the directories, diff the files that were found to have mismatches"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name(CMD_DIFF_STATE)
                        .about("Diff the final state of two accounts hash cache directories")
                        .long_about(
                            "Diff the final state of two accounts hash cache directories. \
                             Load all the latest entries from each directory, then compare \
                             the final states for anything missing or mismatching."
                        )
                        .arg(
                            Arg::with_name("path1")
                                .index(1)
                                .takes_value(true)
                                .value_name("PATH1")
                                .help("Accounts hash cache directory 1 to diff"),
                        )
                        .arg(
                            Arg::with_name("path2")
                                .index(2)
                                .takes_value(true)
                                .value_name("PATH2")
                                .help("Accounts hash cache directory 2 to diff"),
                        )
                        .arg(
                            Arg::with_name("bins")
                                .long("bins")
                                .takes_value(true)
                                .value_name("NUM")
                                .default_value(DEFAULT_BINS)
                                .help("Sets the number of bins to split the entries into")
                                .long_help(
                                    "Sets the number of bins to split the entries into. \
                                     The binning is based on each entry's pubkey. \
                                     Must be a power of two, greater than 0, \
                                     and less-than-or-equal-to 16,777,216 (2^24)"
                                ),
                        )
                        .arg(
                            Arg::with_name("bin_of_interest")
                                .long("bin-of-interest")
                                .takes_value(true)
                                .value_name("INDEX")
                                .help("Specifies a single bin to diff")
                                .long_help(
                                    "Specifies a single bin to diff. \
                                     When diffing large state that does not fit in memory, \
                                     it may be neccessary to diff a subset at a time. \
                                     Use this arg to limit the state to a single bin. \
                                     The INDEX must be less than --bins."
                                ),
                        ),
                ),
        )
        .get_matches();

    let subcommand = matches.subcommand();
    let subcommand_str = subcommand.0;
    match subcommand {
        (CMD_INSPECT, Some(subcommand_matches)) => cmd_inspect(&matches, subcommand_matches),
        (CMD_DIFF, Some(subcommand_matches)) => {
            let diff_subcommand = subcommand_matches.subcommand();
            match diff_subcommand {
                (CMD_DIFF_FILES, Some(diff_subcommand_matches)) => {
                    cmd_diff_files(&matches, diff_subcommand_matches)
                }
                (CMD_DIFF_DIRS, Some(diff_subcommand_matches)) => {
                    cmd_diff_dirs(&matches, diff_subcommand_matches)
                }
                (CMD_DIFF_STATE, Some(diff_subcommand_matches)) => {
                    cmd_diff_state(&matches, diff_subcommand_matches)
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }
    .unwrap_or_else(|err| {
        eprintln!("Error: '{subcommand_str}' failed: {err}");
        std::process::exit(1);
    });
}

fn cmd_inspect(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let force = subcommand_matches.is_present("force");
    let path = value_t_or_exit!(subcommand_matches, "path", String);
    do_inspect(path, force)
}

fn cmd_diff_files(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let path1 = value_t_or_exit!(subcommand_matches, "path1", String);
    let path2 = value_t_or_exit!(subcommand_matches, "path2", String);
    do_diff_files(path1, path2)
}

fn cmd_diff_dirs(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let path1 = value_t_or_exit!(subcommand_matches, "path1", String);
    let path2 = value_t_or_exit!(subcommand_matches, "path2", String);
    let then_diff_files = subcommand_matches.is_present("then_diff_files");
    do_diff_dirs(path1, path2, then_diff_files)
}

fn cmd_diff_state(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let path1 = value_t_or_exit!(subcommand_matches, "path1", String);
    let path2 = value_t_or_exit!(subcommand_matches, "path2", String);
    let num_bins = value_t_or_exit!(subcommand_matches, "bins", usize);
    let bin_of_interest =
        if let Some(bin_of_interest) = subcommand_matches.value_of("bin_of_interest") {
            let bin_of_interest = bin_of_interest
                .parse()
                .map_err(|err| format!("argument 'bin-of-interest' is not a valid value: {err}"))?;
            if bin_of_interest >= num_bins {
                return Err(format!(
                    "argument 'bin-of-interest' must be less than 'bins', \
                     bins: {num_bins}, bin-of-interest: {bin_of_interest}",
                ));
            }
            Some(bin_of_interest)
        } else {
            None
        };
    do_diff_state(path1, path2, num_bins, bin_of_interest)
}

fn do_inspect(file: impl AsRef<Path>, force: bool) -> Result<(), String> {
    let (reader, header) = open_file(&file, force).map_err(|err| {
        format!(
            "failed to open accounts hash cache file '{}': {err}",
            file.as_ref().display(),
        )
    })?;
    let count_width = width10(header.count as u64);

    let mut count = Saturating(0);
    scan_file(reader, header.count, |entry| {
        println!(
            "{count:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {}",
            entry.pubkey.to_string(),
            entry.hash.0.to_string(),
            entry.lamports,
        );
        count += 1;
    })?;

    println!("actual entries: {count}, expected: {}", header.count);
    Ok(())
}

fn do_diff_files(file1: impl AsRef<Path>, file2: impl AsRef<Path>) -> Result<(), String> {
    let LatestEntriesInfo {
        latest_entries: entries1,
        capitalization: capitalization1,
    } = extract_latest_entries_in(&file1)
        .map_err(|err| format!("failed to extract entries from file 1: {err}"))?;
    let LatestEntriesInfo {
        latest_entries: entries2,
        capitalization: capitalization2,
    } = extract_latest_entries_in(&file2)
        .map_err(|err| format!("failed to extract entries from file 2: {err}"))?;

    let num_accounts1 = entries1.len();
    let num_accounts2 = entries2.len();
    let num_accounts_width = {
        let width1 = width10(num_accounts1 as u64);
        let width2 = width10(num_accounts2 as u64);
        cmp::max(width1, width2)
    };
    let lamports_width = {
        let width1 = width10(capitalization1);
        let width2 = width10(capitalization2);
        cmp::max(width1, width2)
    };
    println!("File 1: number of accounts: {num_accounts1:num_accounts_width$}, capitalization: {capitalization1:lamports_width$} lamports");
    println!("File 2: number of accounts: {num_accounts2:num_accounts_width$}, capitalization: {capitalization2:lamports_width$} lamports");

    // compute the differences between the files
    let do_compute = |lhs: &HashMap<_, (_, _)>, rhs: &HashMap<_, (_, _)>| {
        let mut unique_entries = Vec::new();
        let mut mismatch_entries = Vec::new();
        for (lhs_key, lhs_value) in lhs.iter() {
            if let Some(rhs_value) = rhs.get(lhs_key) {
                if lhs_value != rhs_value {
                    mismatch_entries.push((
                        CacheHashDataFileEntry {
                            hash: lhs_value.0,
                            lamports: lhs_value.1,
                            pubkey: *lhs_key,
                        },
                        CacheHashDataFileEntry {
                            hash: rhs_value.0,
                            lamports: rhs_value.1,
                            pubkey: *lhs_key,
                        },
                    ));
                }
            } else {
                unique_entries.push(CacheHashDataFileEntry {
                    hash: lhs_value.0,
                    lamports: lhs_value.1,
                    pubkey: *lhs_key,
                });
            }
        }
        unique_entries.sort_unstable_by(|a, b| a.pubkey.cmp(&b.pubkey));
        mismatch_entries.sort_unstable_by(|a, b| a.0.pubkey.cmp(&b.0.pubkey));
        (unique_entries, mismatch_entries)
    };
    let (unique_entries1, mismatch_entries) = do_compute(&entries1, &entries2);
    let (unique_entries2, _) = do_compute(&entries2, &entries1);

    println!("Unique entries in file 1:");
    print_unique_entries(&unique_entries1, lamports_width);
    println!("Unique entries in file 2:");
    print_unique_entries(&unique_entries2, lamports_width);

    println!("Mismatch values:");
    let count_width = width10(mismatch_entries.len() as u64);
    if mismatch_entries.is_empty() {
        println!("(none)");
    } else {
        for (i, (lhs, rhs)) in mismatch_entries.iter().enumerate() {
            println!(
                "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {:lamports_width$}",
                lhs.pubkey.to_string(),
                lhs.hash.0.to_string(),
                lhs.lamports,
            );
            println!(
                "{i:count_width$}: file 2: {:44}, hash: {:44}, lamports: {:lamports_width$}",
                "(same)".to_string(),
                rhs.hash.0.to_string(),
                rhs.lamports,
            );
        }
    }

    Ok(())
}

fn do_diff_dirs(
    dir1: impl AsRef<Path>,
    dir2: impl AsRef<Path>,
    then_diff_files: bool,
) -> Result<(), String> {
    let _timer = ElapsedOnDrop::new("diffing directories took ");

    let files1 = get_cache_files_in(dir1)
        .map_err(|err| format!("failed to get cache files in dir1: {err}"))?;
    let files2 = get_cache_files_in(dir2)
        .map_err(|err| format!("failed to get cache files in dir2: {err}"))?;

    let mut uniques1 = Vec::new();
    let mut uniques2 = Vec::new();
    let mut mismatches = Vec::new();
    let mut idx1 = Saturating(0);
    let mut idx2 = Saturating(0);
    while idx1.0 < files1.len() && idx2.0 < files2.len() {
        let file1 = &files1[idx1.0];
        let file2 = &files2[idx2.0];
        match file1
            .parsed
            .slot_range_start
            .cmp(&file2.parsed.slot_range_start)
        {
            Ordering::Less => {
                // file1 is an older slot range than file2, so note it and advance idx1
                uniques1.push(file1);
                idx1 += 1;
            }
            Ordering::Greater => {
                // file1 is an newer slot range than file2, so note it and advance idx2
                uniques2.push(file2);
                idx2 += 1;
            }
            Ordering::Equal => {
                match file1
                    .parsed
                    .slot_range_end
                    .cmp(&file2.parsed.slot_range_end)
                {
                    Ordering::Less => {
                        // file1 is a smaller slot range than file2, so note it and advance idx1
                        uniques1.push(file1);
                        idx1 += 1;
                    }
                    Ordering::Greater => {
                        // file1 is a larger slot range than file2, so note it and advance idx2
                        uniques2.push(file2);
                        idx2 += 1;
                    }
                    Ordering::Equal => {
                        // slot ranges match, so compare the files and advance both idx1 and idx2
                        let is_equal = || {
                            // if the files have different sizes, they are not equal
                            if file1.metadata.len() != file2.metadata.len() {
                                return false;
                            }

                            // if the parsed file names have different hashes, they are not equal
                            if file1.parsed.hash != file2.parsed.hash {
                                return false;
                            }

                            // if the file headers have different entry counts, they are not equal
                            let Ok((mmap1, header1)) = mmap_file(&file1.path, false) else {
                                return false;
                            };
                            let Ok((mmap2, header2)) = mmap_file(&file2.path, false) else {
                                return false;
                            };
                            if header1.count != header2.count {
                                return false;
                            }

                            // if the binary data of the files are different, they are not equal
                            let hasher = RandomState::new();
                            let hash1 = hasher.hash_one(mmap1.as_ref());
                            let hash2 = hasher.hash_one(mmap2.as_ref());
                            if hash1 != hash2 {
                                return false;
                            }

                            // ...otherwise they are equal
                            true
                        };
                        if !is_equal() {
                            mismatches.push((file1, file2));
                        }
                        idx1 += 1;
                        idx2 += 1;
                    }
                }
            }
        }
    }

    for file in files1.iter().skip(idx1.0) {
        uniques1.push(file);
    }
    for file in files2.iter().skip(idx2.0) {
        uniques2.push(file);
    }

    let do_print = |entries: &[&CacheFileInfo]| {
        let count_width = width10(entries.len() as u64);
        if entries.is_empty() {
            println!("(none)");
        } else {
            for (i, entry) in entries.iter().enumerate() {
                println!("{i:count_width$}: '{}'", entry.path.display());
            }
        }
    };
    println!("Unique files in directory 1:");
    do_print(&uniques1);
    println!("Unique files in directory 2:");
    do_print(&uniques2);

    println!("Mismatch files:");
    let count_width = width10(mismatches.len() as u64);
    if mismatches.is_empty() {
        println!("(none)");
    } else {
        for (i, (file1, file2)) in mismatches.iter().enumerate() {
            println!(
                "{i:count_width$}: '{}', '{}'",
                file1.path.display(),
                file2.path.display(),
            );
        }
        if then_diff_files {
            for (file1, file2) in &mismatches {
                println!(
                    "Differences between '{}' and '{}':",
                    file1.path.display(),
                    file2.path.display(),
                );
                if let Err(err) = do_diff_files(&file1.path, &file2.path) {
                    eprintln!("Error: failed to diff files: {err}");
                }
            }
        }
    }

    Ok(())
}

fn do_diff_state(
    dir1: impl AsRef<Path>,
    dir2: impl AsRef<Path>,
    num_bins: usize,
    bin_of_interest: Option<usize>,
) -> Result<(), String> {
    let extract = |dir: &Path| -> Result<_, String> {
        let files =
            get_cache_files_in(dir).map_err(|err| format!("failed to get cache files: {err}"))?;
        let BinnedLatestEntriesInfo {
            latest_entries,
            capitalization,
        } = extract_binned_latest_entries_in(
            files.iter().map(|file| &file.path),
            num_bins,
            bin_of_interest,
        )
        .map_err(|err| format!("failed to extract entries: {err}"))?;
        let num_accounts: usize = latest_entries.iter().map(|bin| bin.len()).sum();
        let entries = Vec::from(latest_entries);
        let state: Box<_> = entries.into_iter().map(RwLock::new).collect();
        Ok((state, capitalization, num_accounts))
    };

    let timer = LoggingTimer::new("Reconstructing state");
    let dir1 = dir1.as_ref();
    let dir2 = dir2.as_ref();
    let (state1, state2) = rayon::join(|| extract(dir1), || extract(dir2));
    let (state1, capitalization1, num_accounts1) = state1
        .map_err(|err| format!("failed to get state for dir 1 '{}': {err}", dir1.display()))?;
    let (state2, capitalization2, num_accounts2) = state2
        .map_err(|err| format!("failed to get state for dir 2 '{}': {err}", dir2.display()))?;
    drop(timer);

    let timer = LoggingTimer::new("Diffing state");
    let (mut mismatch_entries, mut unique_entries1) = (0..num_bins)
        .into_par_iter()
        .map(|bindex| {
            let mut bin1 = state1[bindex].write().unwrap();
            let mut bin2 = state2[bindex].write().unwrap();

            let mut mismatch_entries = Vec::new();
            let mut unique_entries1 = Vec::new();
            for entry1 in bin1.drain() {
                let (key1, value1) = entry1;
                match bin2.remove(&key1) {
                    Some(value2) => {
                        // the pubkey was found in both states, so compare the hashes and lamports
                        if value1 == value2 {
                            // hashes and lamports are equal, so nothing to do
                        } else {
                            // otherwise we have a mismatch; note it
                            mismatch_entries.push((key1, value1, value2));
                        }
                    }
                    None => {
                        // this pubkey was *not* found in state2, so its a unique entry in state1
                        unique_entries1.push(CacheHashDataFileEntry {
                            pubkey: key1,
                            hash: value1.0,
                            lamports: value1.1,
                        });
                    }
                }
            }
            (mismatch_entries, unique_entries1)
        })
        .reduce(
            || (Vec::new(), Vec::new()),
            |mut accum, elem| {
                accum.0.extend(elem.0);
                accum.1.extend(elem.1);
                accum
            },
        );
    drop(timer);

    // all the remaining entries in state2 are the ones *not* found in state1
    let mut unique_entries2 = Vec::new();
    for bin in Vec::from(state2).into_iter() {
        let mut bin = bin.write().unwrap();
        unique_entries2.extend(bin.drain().map(|(pubkey, (hash, lamports))| {
            CacheHashDataFileEntry {
                pubkey,
                hash,
                lamports,
            }
        }));
    }

    // sort all the results by pubkey to make them saner to view
    let timer = LoggingTimer::new("Sorting results");
    unique_entries1.sort_unstable_by(|a, b| a.pubkey.cmp(&b.pubkey));
    unique_entries2.sort_unstable_by(|a, b| a.pubkey.cmp(&b.pubkey));
    mismatch_entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    drop(timer);

    let num_accounts_width = {
        let width1 = width10(num_accounts1 as u64);
        let width2 = width10(num_accounts2 as u64);
        cmp::max(width1, width2)
    };
    let lamports_width = {
        let width1 = width10(capitalization1);
        let width2 = width10(capitalization2);
        cmp::max(width1, width2)
    };

    println!("State 1: total number of accounts: {num_accounts1:num_accounts_width$}, total capitalization: {capitalization1:lamports_width$} lamports");
    println!("State 2: total number of accounts: {num_accounts2:num_accounts_width$}, total capitalization: {capitalization2:lamports_width$} lamports");

    println!("Unique entries in state 1:");
    print_unique_entries(&unique_entries1, lamports_width);
    println!("Unique entries in state 2:");
    print_unique_entries(&unique_entries2, lamports_width);

    println!("Mismatch values:");
    let count_width = width10(mismatch_entries.len() as u64);
    if mismatch_entries.is_empty() {
        println!("(none)");
    } else {
        for (i, (pubkey, value1, value2)) in mismatch_entries.iter().enumerate() {
            println!(
                "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {:lamports_width$}",
                pubkey.to_string(),
                value1.0 .0.to_string(),
                value1.1,
            );
            println!(
                "{i:count_width$}: {:52}, hash: {:44}, lamports: {:lamports_width$}",
                "(state 2 same)",
                value2.0 .0.to_string(),
                value2.1,
            );
        }
    }

    Ok(())
}

/// Returns all the cache hash data files in `dir`, sorted in ascending slot-and-bin-range order
fn get_cache_files_in(dir: impl AsRef<Path>) -> Result<Vec<CacheFileInfo>, io::Error> {
    fn get_files_in(dir: impl AsRef<Path>) -> Result<Vec<(PathBuf, Metadata)>, io::Error> {
        let mut files = Vec::new();
        let entries = fs::read_dir(dir)?;
        for entry in entries {
            let path = entry?.path();
            let meta = fs::metadata(&path)?;
            if meta.is_file() {
                let path = fs::canonicalize(path)?;
                files.push((path, meta));
            }
        }
        Ok(files)
    }

    let files = get_files_in(&dir).map_err(|err| {
        io::Error::other(format!(
            "failed to get files in '{}': {err}",
            dir.as_ref().display(),
        ))
    })?;
    let mut cache_files: Vec<_> = files
        .into_iter()
        .filter_map(|file| {
            Path::file_name(&file.0)
                .and_then(parse_cache_hash_data_filename)
                .map(|parsed_file_name| CacheFileInfo {
                    path: file.0,
                    metadata: file.1,
                    parsed: parsed_file_name,
                })
        })
        .collect();
    cache_files.sort_unstable_by(|a, b| {
        a.parsed
            .slot_range_start
            .cmp(&b.parsed.slot_range_start)
            .then_with(|| a.parsed.slot_range_end.cmp(&b.parsed.slot_range_end))
            .then_with(|| a.parsed.bin_range_start.cmp(&b.parsed.bin_range_start))
            .then_with(|| a.parsed.bin_range_end.cmp(&b.parsed.bin_range_end))
    });
    Ok(cache_files)
}

/// Returns the entries in `file`, and the capitalization
///
/// If there are multiple entries for a pubkey, only the latest is returned.
fn extract_latest_entries_in(file: impl AsRef<Path>) -> Result<LatestEntriesInfo, String> {
    const NUM_BINS: usize = 1;
    let BinnedLatestEntriesInfo {
        latest_entries,
        capitalization,
    } = extract_binned_latest_entries_in(iter::once(file), NUM_BINS, None)?;
    assert_eq!(latest_entries.len(), NUM_BINS);
    let mut latest_entries = Vec::from(latest_entries);
    let latest_entries = latest_entries.pop().unwrap();

    Ok(LatestEntriesInfo {
        latest_entries,
        capitalization,
    })
}

/// Returns the entries in `files`, binned by pubkey, and the capitalization
///
/// If there are multiple entries for a pubkey, only the latest is returned.
///
/// - `num_bins` specifies the number of bins to split the entries into.
/// - `bin_of_interest`, if Some, only returns entries from that bin.
///   Otherwise, returns all entries.  Must be less than `num_bins`.
///
/// Note: `files` must be sorted in ascending order, as insertion order is
/// relied on to guarantee the latest entry is returned.
fn extract_binned_latest_entries_in(
    files: impl IntoIterator<Item = impl AsRef<Path>>,
    num_bins: usize,
    bin_of_interest: Option<usize>,
) -> Result<BinnedLatestEntriesInfo, String> {
    if let Some(bin_of_interest) = bin_of_interest {
        assert!(bin_of_interest < num_bins);
    }

    let binner = PubkeyBinCalculator24::new(num_bins);
    let mut entries: Box<_> = iter::repeat_with(HashMap::default).take(num_bins).collect();
    let mut capitalization = Saturating(0);

    for file in files.into_iter() {
        let force = false; // skipping sanity checks is not supported when extracting entries
        let (mmap, header) = mmap_file(&file, force).map_err(|err| {
            format!(
                "failed to open accounts hash cache file '{}': {err}",
                file.as_ref().display(),
            )
        })?;

        let num_entries = scan_mmap(&mmap, |entry| {
            let bin = binner.bin_from_pubkey(&entry.pubkey);
            if let Some(bin_of_interest) = bin_of_interest {
                // Is this the bin of interest? If not, skip it.
                if bin != bin_of_interest {
                    return;
                }
            }

            capitalization += entry.lamports;
            let old_value = entries[bin].insert(entry.pubkey, (entry.hash, entry.lamports));
            if let Some((_, old_lamports)) = old_value {
                // back out the old value's lamports, so we only keep the latest's for capitalization
                capitalization -= old_lamports;
            }
        });

        if num_entries != header.count {
            return Err(format!(
                "mismatched number of entries when scanning '{}': expected: {}, actual: {num_entries}",
                file.as_ref().display(), header.count,
            ));
        }
    }

    Ok(BinnedLatestEntriesInfo {
        latest_entries: entries,
        capitalization: capitalization.0,
    })
}

/// Scans `mmap` and applies `user_fn` to each entry
fn scan_mmap(mmap: &Mmap, mut user_fn: impl FnMut(&CacheHashDataFileEntry)) -> usize {
    const SIZE_OF_ENTRY: usize = size_of::<CacheHashDataFileEntry>();
    let bytes = &mmap[size_of::<CacheHashDataFileHeader>()..];
    let mut num_entries = Saturating(0);
    for chunk in bytes.chunks_exact(SIZE_OF_ENTRY) {
        let entry = bytemuck::from_bytes(chunk);
        user_fn(entry);
        num_entries += 1;
    }
    num_entries.0
}

/// Scans file with `reader` and applies `user_fn` to each entry
///
/// NOTE: `reader`'s cursor must already be at the first entry; i.e. *past* the header.
fn scan_file(
    mut reader: impl Read,
    num_entries_expected: usize,
    mut user_fn: impl FnMut(CacheHashDataFileEntry),
) -> Result<(), String> {
    let mut num_entries_actual = Saturating(0);
    let mut entry = CacheHashDataFileEntry::zeroed();
    loop {
        let result = reader.read_exact(bytemuck::bytes_of_mut(&mut entry));
        match result {
            Ok(()) => {}
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof
                    && num_entries_actual.0 == num_entries_expected
                {
                    // we've hit the expected end of the file
                    break;
                } else {
                    return Err(format!(
                        "failed to read file entry {num_entries_actual}, \
                         expected {num_entries_expected} entries: {err}",
                    ));
                }
            }
        };
        user_fn(entry);
        num_entries_actual += 1;
    }
    Ok(())
}

fn mmap_file(
    path: impl AsRef<Path>,
    force: bool,
) -> Result<(Mmap, CacheHashDataFileHeader), String> {
    let (reader, header) = open_file(&path, force)?;
    let file = reader.into_inner();
    let mmap = unsafe { Mmap::map(&file) }
        .map_err(|err| format!("failed to mmap '{}': {err}", path.as_ref().display()))?;
    Ok((mmap, header))
}

fn open_file(
    path: impl AsRef<Path>,
    force: bool,
) -> Result<(BufReader<File>, CacheHashDataFileHeader), String> {
    let file = File::open(path).map_err(|err| format!("{err}"))?;
    let actual_file_size = file
        .metadata()
        .map_err(|err| format!("failed to query file metadata: {err}"))?
        .len();
    let mut reader = BufReader::new(file);

    let header = {
        let mut header = CacheHashDataFileHeader::zeroed();
        reader
            .read_exact(bytemuck::bytes_of_mut(&mut header))
            .map_err(|err| format!("failed to read header: {err}"))?;
        header
    };

    // Sanity checks -- ensure the actual file size matches the expected file size
    let expected_file_size = size_of::<CacheHashDataFileHeader>()
        .saturating_add(size_of::<CacheHashDataFileEntry>().saturating_mul(header.count));
    if actual_file_size != expected_file_size as u64 {
        let err_msg = format!(
            "failed sanitization: actual file size does not match expected file size! \
             actual: {actual_file_size}, expected: {expected_file_size}",
        );
        if force {
            eprintln!("Warning: {err_msg}\nForced. Continuing... Results may be incorrect.");
        } else {
            return Err(err_msg);
        }
    }

    Ok((reader, header))
}

/// Prints unique entries
fn print_unique_entries(entries: &[CacheHashDataFileEntry], lamports_width: usize) {
    if entries.is_empty() {
        println!("(none)");
        return;
    }

    let count_width = width10(entries.len() as u64);
    let mut total_lamports = Saturating(0);
    for (i, entry) in entries.iter().enumerate() {
        total_lamports += entry.lamports;
        println!(
            "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {:lamports_width$}",
            entry.pubkey.to_string(),
            entry.hash.0.to_string(),
            entry.lamports,
        );
    }
    println!("total lamports: {}", total_lamports.0);
}

/// Returns the number of characters required to print `x` in base-10
fn width10(x: u64) -> usize {
    (x as f64).log10().ceil() as usize
}

#[derive(Debug)]
struct CacheFileInfo {
    path: PathBuf,
    metadata: Metadata,
    parsed: ParsedCacheHashDataFilename,
}

#[derive(Debug)]
struct LatestEntriesInfo {
    latest_entries: HashMap<Pubkey, (AccountHash, /* lamports */ u64)>,
    capitalization: u64, // lamports
}

#[derive(Debug)]
struct BinnedLatestEntriesInfo {
    latest_entries: Box<[HashMap<Pubkey, (AccountHash, /* lamports */ u64)>]>,
    capitalization: u64, // lamports
}

#[derive(Debug)]
struct LoggingTimer {
    _elapsed_on_drop: ElapsedOnDrop,
}

impl LoggingTimer {
    #[must_use]
    fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let elapsed_on_drop = ElapsedOnDrop {
            message: format!("{message}... Done in "),
            start: Instant::now(),
        };
        println!("{message}...");
        Self {
            _elapsed_on_drop: elapsed_on_drop,
        }
    }
}

#[derive(Debug)]
struct ElapsedOnDrop {
    message: String,
    start: Instant,
}

impl ElapsedOnDrop {
    #[must_use]
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            start: Instant::now(),
        }
    }
}

impl Drop for ElapsedOnDrop {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        println!("{}{elapsed:?}", self.message);
    }
}
