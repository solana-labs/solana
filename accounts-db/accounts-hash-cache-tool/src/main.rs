use {
    bytemuck::Zeroable as _,
    clap::{
        crate_description, crate_name, value_t_or_exit, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    solana_accounts_db::{CacheHashDataFileEntry, CacheHashDataFileHeader},
    std::{
        collections::HashMap,
        fs::File,
        io::{self, BufReader, Read as _},
        mem::size_of,
        num::Saturating,
        path::Path,
    },
};

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
            SubCommand::with_name("inspect")
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
            SubCommand::with_name("diff")
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
        .get_matches();

    match matches.subcommand() {
        ("inspect", Some(subcommand_matches)) => do_inspect(&matches, subcommand_matches)
            .map_err(|err| format!("inspection failed: {err}")),
        ("diff", Some(subcommand_matches)) => {
            do_diff(&matches, subcommand_matches).map_err(|err| format!("diff failed: {err}"))
        }
        _ => unreachable!(),
    }
    .unwrap_or_else(|err| {
        eprintln!("Error: {err}");
        std::process::exit(1);
    });
}

fn do_inspect(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let force = subcommand_matches.is_present("force");
    let path = value_t_or_exit!(subcommand_matches, "path", String);
    let (mut reader, header) = open_file(&path, force)
        .map_err(|err| format!("failed to open accounts hash cache file '{path}': {err}"))?;
    let count_width = (header.count as f64).log10().ceil() as usize;
    let mut count = Saturating(0usize);
    loop {
        let mut entry = CacheHashDataFileEntry::zeroed();
        let result = reader.read_exact(bytemuck::bytes_of_mut(&mut entry));
        match result {
            Ok(()) => {}
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof && count.0 == header.count {
                    // we've hit the expected end of the file
                    break;
                } else {
                    return Err(format!(
                        "failed to read entry {count}, expected {}: {err}",
                        header.count,
                    ));
                }
            }
        };
        println!(
            "{count:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {}",
            entry.pubkey.to_string(),
            entry.hash.0.to_string(),
            entry.lamports,
        );
        count += 1;
    }

    println!("actual entries: {count}, expected: {}", header.count);
    Ok(())
}

fn do_diff(
    _app_matches: &ArgMatches<'_>,
    subcommand_matches: &ArgMatches<'_>,
) -> Result<(), String> {
    let force = false; // skipping sanity checks is not supported when diffing
    let path1 = value_t_or_exit!(subcommand_matches, "path1", String);
    let path2 = value_t_or_exit!(subcommand_matches, "path2", String);
    let (mut reader1, header1) = open_file(&path1, force)
        .map_err(|err| format!("failed to open accounts hash cache file 1 '{path1}': {err}"))?;
    let (mut reader2, header2) = open_file(&path2, force)
        .map_err(|err| format!("failed to open accounts hash cache file 2 '{path2}': {err}"))?;
    // Note: Purposely open both files before reading either one.  This way, if there's an error
    // opening file 2, we can bail early without having to wait for file 1 to be read completely.

    // extract the entries from both files
    let do_extract = |num, reader: &mut BufReader<_>, header: &CacheHashDataFileHeader| {
        let mut entries = HashMap::<_, _>::default();
        loop {
            let mut entry = CacheHashDataFileEntry::zeroed();
            let result = reader.read_exact(bytemuck::bytes_of_mut(&mut entry));
            match result {
                Ok(()) => {}
                Err(err) => {
                    if err.kind() == io::ErrorKind::UnexpectedEof && entries.len() == header.count {
                        // we've hit the expected end of the file
                        break;
                    } else {
                        return Err(format!(
                            "failed to read entry {}, expected {}: {err}",
                            entries.len(),
                            header.count,
                        ));
                    }
                }
            };
            let CacheHashDataFileEntry {
                hash,
                lamports,
                pubkey,
            } = entry;
            let old_value = entries.insert(pubkey, (hash, lamports));
            if let Some(old_value) = old_value {
                let new_value = entries.get(&pubkey);
                return Err(format!("found duplicate pubkey in file {num}: {pubkey}, old value: {old_value:?}, new value: {new_value:?}"));
            }
        }
        Ok(entries)
    };
    let entries1 = do_extract(1, &mut reader1, &header1)?;
    let entries2 = do_extract(2, &mut reader2, &header2)?;

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
            for (i, entry) in entries.iter().enumerate() {
                println!(
                    "{i:count_width$}: pubkey: {:44}, hash: {:44}, lamports: {}",
                    entry.pubkey.to_string(),
                    entry.hash.0.to_string(),
                    entry.lamports,
                );
            }
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
