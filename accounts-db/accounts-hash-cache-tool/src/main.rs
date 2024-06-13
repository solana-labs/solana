use {
    bytemuck::Zeroable as _,
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg},
    solana_accounts_db::{CacheHashDataFileEntry, CacheHashDataFileHeader},
    std::{
        fs::File,
        io::{self, BufReader, Read as _},
        mem::size_of,
        num::Saturating,
    },
};

fn main() {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("path")
                .index(1)
                .takes_value(true)
                .value_name("PATH")
                .help("Accounts hash cache file to inspect"),
        )
        .arg(
            Arg::with_name("force")
                .long("force")
                .takes_value(false)
                .help("Continue even if sanity checks fail"),
        )
        .get_matches();

    let force = matches.is_present("force");
    let path = value_t_or_exit!(matches, "path", String);

    let file = File::open(&path).unwrap_or_else(|err| {
        eprintln!("Failed to open accounts hash cache file '{path}': {err}");
        std::process::exit(1);
    });
    let actual_file_size = file
        .metadata()
        .unwrap_or_else(|err| {
            eprintln!("Failed to query file metadata: {err}");
            std::process::exit(1);
        })
        .len();
    let mut reader = BufReader::new(file);

    let header = {
        let mut header = CacheHashDataFileHeader::zeroed();
        reader
            .read_exact(bytemuck::bytes_of_mut(&mut header))
            .unwrap_or_else(|err| {
                eprintln!("Failed to read header: {err}");
                std::process::exit(1);
            });
        header
    };

    // Sanity checks -- ensure the actual file size matches the expected file size
    let expected_file_size = size_of::<CacheHashDataFileHeader>()
        .saturating_add(size_of::<CacheHashDataFileEntry>().saturating_mul(header.count));
    if actual_file_size != expected_file_size as u64 {
        eprintln!(
            "Failed sanitization: actual file size does not match expected file size! \
             actual: {actual_file_size}, expected: {expected_file_size}",
        );
        if !force {
            std::process::exit(1);
        }
        eprintln!("Forced. Continuing... Results may be incorrect.");
    }

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
                } else {
                    eprintln!("Failed to read entry {count}: {err}");
                }
                break;
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
}
