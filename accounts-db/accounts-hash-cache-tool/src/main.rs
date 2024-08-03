use {
    ahash::{HashMap, RandomState},
    bytemuck::Zeroable as _,
    clap::{
        crate_description, crate_name, value_t_or_exit, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    memmap2::Mmap,
    solana_accounts_db::{
        parse_cache_hash_data_filename, CacheHashDataFileEntry, CacheHashDataFileHeader,
        ParsedCacheHashDataFilename,
    },
    std::{
        cmp::Ordering,
        fs::{self, File, Metadata},
        io::{self, BufReader, Read},
        mem::size_of,
        num::Saturating,
        path::{Path, PathBuf},
        time::Instant,
    },
};

const CMD_INSPECT: &str = "inspect";
const CMD_DIFF: &str = "diff";
const CMD_DIFF_FILES: &str = "files";
const CMD_DIFF_DIRS: &str = "directories";

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

fn do_inspect(file: impl AsRef<Path>, force: bool) -> Result<(), String> {
    let (reader, header) = open_file(&file, force).map_err(|err| {
        format!(
            "failed to open accounts hash cache file '{}': {err}",
            file.as_ref().display(),
        )
    })?;
    let count_width = (header.count as f64).log10().ceil() as usize;

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
    let force = false; // skipping sanity checks is not supported when diffing
    let (mut reader1, header1) = open_file(&file1, force).map_err(|err| {
        format!(
            "failed to open accounts hash cache file 1 '{}': {err}",
            file1.as_ref().display(),
        )
    })?;
    let (mut reader2, header2) = open_file(&file2, force).map_err(|err| {
        format!(
            "failed to open accounts hash cache file 2 '{}': {err}",
            file2.as_ref().display(),
        )
    })?;
    // Note: Purposely open both files before reading either one.  This way, if there's an error
    // opening file 2, we can bail early without having to wait for file 1 to be read completely.

    // extract the entries from both files
    let do_extract = |reader: &mut BufReader<_>, header: &CacheHashDataFileHeader| {
        let mut entries = Vec::new();
        scan_file(reader, header.count, |entry| {
            entries.push(entry);
        })?;

        // entries in the file are sorted by pubkey then slot,
        // so we want to keep the *last* entry (if there are duplicates)
        let entries: HashMap<_, _> = entries
            .into_iter()
            .map(|entry| (entry.pubkey, (entry.hash, entry.lamports)))
            .collect();
        Ok::<_, String>(entries)
    };
    let entries1 = do_extract(&mut reader1, &header1)
        .map_err(|err| format!("failed to extract entries from file 1: {err}"))?;
    let entries2 = do_extract(&mut reader2, &header2)
        .map_err(|err| format!("failed to extract entries from file 2: {err}"))?;

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

    // display the unique entries in each file
    let do_print = |entries: &[CacheHashDataFileEntry]| {
        let count_width = (entries.len() as f64).log10().ceil() as usize;
        if entries.is_empty() {
            println!("(none)");
        } else {
            let mut total_lamports = Saturating(0);
            for (i, entry) in entries.iter().enumerate() {
                total_lamports += entry.lamports;
                println!(
                    "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {}",
                    entry.pubkey.to_string(),
                    entry.hash.0.to_string(),
                    entry.lamports,
                );
            }
            println!("total lamports: {}", total_lamports.0);
        }
    };
    println!("Unique entries in file 1:");
    do_print(&unique_entries1);
    println!("Unique entries in file 2:");
    do_print(&unique_entries2);

    println!("Mismatch values:");
    let count_width = (mismatch_entries.len() as f64).log10().ceil() as usize;
    if mismatch_entries.is_empty() {
        println!("(none)");
    } else {
        for (i, (lhs, rhs)) in mismatch_entries.iter().enumerate() {
            println!(
                "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {}",
                lhs.pubkey.to_string(),
                lhs.hash.0.to_string(),
                lhs.lamports,
            );
            println!(
                "{i:count_width$}: file 2: {:44}, hash: {:44}, lamports: {}",
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
    let _timer = ElapsedOnDrop {
        message: "diffing directories took ".to_string(),
        start: Instant::now(),
    };

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
                            let Ok((mmap1, header1)) = map_file(&file1.path, false) else {
                                return false;
                            };
                            let Ok((mmap2, header2)) = map_file(&file2.path, false) else {
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
        let count_width = (entries.len() as f64).log10().ceil() as usize;
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
    let count_width = (mismatches.len() as f64).log10().ceil() as usize;
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

/// Scan file with `reader` and apply `user_fn` to each entry
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

fn map_file(
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

#[derive(Debug)]
struct CacheFileInfo {
    path: PathBuf,
    metadata: Metadata,
    parsed: ParsedCacheHashDataFilename,
}

#[derive(Debug)]
struct ElapsedOnDrop {
    message: String,
    start: Instant,
}

impl Drop for ElapsedOnDrop {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        println!("{}{elapsed:?}", self.message);
    }
}
