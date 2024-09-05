use std::path::Path;

use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg},
    solana_accounts_db::append_vec::AppendVec,
    solana_sdk::{account::ReadableAccount, system_instruction::MAX_PERMITTED_DATA_LENGTH},
    std::{mem::ManuallyDrop, num::Saturating},
};

struct Bin {
    slot_min: usize,
    slot_max: usize,
    count: usize,
    min_size: usize,
    max_size: usize,
    sum_size: usize,
    avg: usize,
}

fn pad(width: usize) -> String {
    let mut s = format!("");
    for i in 0..width {
        s = format!("{s} ");
    }
    s
}

fn get_stars(x: usize, max: usize, width: usize) -> String {
    let mut s = format!("");
    let percent = x * width / max;
    for i in 0..width {
        s = format!("{s}{}", if i <= percent { "*" } else { " " });
    }
    s
}

fn calc(info: &Vec<(usize, usize)>, bin_widths: Vec<usize>) {
    let mut info = info.clone();
    info.sort();
    let min = info.first().unwrap().0;
    let max_inclusive = info.last().unwrap().0;
    eprintln!("storages: {}", info.len());
    eprintln!("lowest slot: {min}");
    eprintln!("highest slot: {max_inclusive}");
    eprintln!("slot range: {}", max_inclusive - min + 1);
    eprintln!(
        "outside of epoch: {}",
        info.iter()
            .filter(|x| x.0 < max_inclusive - 432_000)
            .count()
    );

    let mut bins = Vec::default();
    for i in 0..bin_widths.len() {
        let next = if i == bin_widths.len() - 1 {
            usize::MAX
        } else {
            bin_widths[i + 1]
        };
        let mut abin = Bin {
            slot_min: bin_widths[i],
            slot_max: next,
            count: 0,
            min_size: usize::MAX,
            max_size: 0,
            sum_size: 0,
            avg: 0,
        };
        bins.push(abin);
    }
    let mut bin_all = Bin {
        slot_min: 0,
        slot_max: 0,
        count: 0,
        min_size: usize::MAX,
        max_size: 0,
        sum_size: 0,
        avg: 0,
    };
    let mut bin_max = Bin {
        slot_min: 0,
        slot_max: 0,
        count: 0,
        min_size: 0,
        max_size: 0,
        sum_size: 0,
        avg: 0,
    };
    info.into_iter().for_each(|(slot, size)| {
        for bin in bins.iter_mut() {
            let relative = max_inclusive - slot;
            if bin.slot_min <= relative && bin.slot_max > relative {
                // eprintln!("{}, {}, {}, {}", slot, relative, max_inclusive, bin.slot_min);
                bin.count += 1;
                bin.sum_size += size;
                bin.min_size = bin.min_size.min(size);
                bin.max_size = bin.max_size.max(size);

                bin_all.count += 1;
                bin_all.sum_size += size;
                bin_all.min_size = bin_all.min_size.min(size);
                bin_all.max_size = bin_all.max_size.max(size);

                break;
            }
        }
    });
    bins.retain_mut(|bin| {
        if bin.count > 0 {
            bin_max.sum_size = bin_max.sum_size.max(bin.sum_size);
            bin_max.max_size = bin_max.max_size.max(bin.max_size);
            bin_max.count = bin_max.count.max(bin.count);
            bin_max.min_size = bin_max.min_size.max(bin.min_size);
            bin.avg = bin.sum_size / bin.count;
        }
        bin_max.avg = bin_max.avg.max(bin.avg);

        bin.count > 0
    });

    bin_all.avg = bin_all.sum_size / bin_all.count;

    eprintln!("overall stats");
    eprintln!("size {}", bin_all.sum_size);
    eprintln!("count {}", bin_all.count);
    eprintln!("min size {}", bin_all.min_size);
    eprintln!("max size {}", bin_all.max_size);
    eprintln!("avg size {}", bin_all.sum_size / bin_all.count);
    eprintln!("avg size {}", bin_all.avg);
    eprintln!("bin width {}", bins[0].slot_max - bins[0].slot_min);
    eprintln!("...");

    for i in 0..bins.len() {
        if i > 0 && bins[i - 1].slot_max != bins[i].slot_min {
            eprintln!("...");
        }
        let bin = &bins[i];
        if bin.slot_min == 432_000 {
            eprintln!("-------------------------------------------------------------------------------------------------------------------------------------------------------------------");
        }
        let mut offset = format!("{:10}", bin.slot_min);

        if i == 0 {
            let s = [
                format!("{:10}", "slot age"),
                pad(2),
                format!("{:10}", "count"),
                pad(2),
                format!("{:10}", "min size"),
                pad(2),
                format!("{:10}", "max size"),
                pad(2),
                format!("{:10}", "sum size"),
                pad(2),
                format!("{:10}", "avg size"),
                pad(2),
                format!(",{:>15}", "slot min"),
                format!(",{:>15}", "count"),
                format!(",{:>15}", "sum size"),
                format!(",{:>15}", "min size"),
                format!(",{:>15}", "max size"),
                format!(",{:>15}", "avg size"),
            ];
            let mut s2 = format!("");
            s.iter().for_each(|s| {
                s2 = format!("{s2}{s}");
            });
            eprintln!("{s2}");
        }

        let s = [
            offset,
            pad(2),
            get_stars(bin.count, bin_max.count, 10),
            pad(2),
            get_stars(bin.min_size, bin_max.min_size, 10),
            pad(2),
            get_stars(bin.max_size, bin_max.max_size, 10),
            pad(2),
            get_stars(bin.sum_size, bin_max.sum_size, 10),
            pad(2),
            get_stars(bin.avg, bin_max.avg, 10),
            pad(2),
            format!(",{:15}", max_inclusive - bin.slot_min),
            format!(",{:15}", bin.count),
            format!(",{:15}", bin.sum_size),
            format!(",{:15}", bin.min_size),
            format!(",{:15}", bin.max_size),
            format!(",{:15}", bin.avg),
        ];
        let mut s2 = format!("");
        s.iter().for_each(|s| {
            s2 = format!("{s2}{s}");
        });
        eprintln!("{s2}");
    }
}

fn normal_bin_widths() -> Vec<usize> {
    let mut bin_widths = vec![0];
    let div = 432_000 / 20;
    for i in 1..432_000 {
        let b = i * div;
        if b > 432_000 {
            break;
        }
        bin_widths.push(b);
    }
    bin_widths.push(432_000);
    for i in 1..100000 {
        let b = 432_000 + i * div;
        // if b > max_range {
            // break;
        // }
        bin_widths.push(b);
    }
    bin_widths
}

fn normal_ancient() -> Vec<usize> {
    let mut bin_widths = vec![0];
    bin_widths.push(432_000);
    bin_widths
}
fn normal_10k() -> Vec<usize> {
    let mut bin_widths = vec![0];
    bin_widths.push(432_000);
    bin_widths.push(442_000);
    bin_widths
}

fn main() {
    if false {
        let info = vec![
            (0, 8usize),
            (500, 23usize),
            (501, 100),
            (432_000 - 1, 2),
            (432_000, 1),
            (500_000, 18),
            (1_000_000, 80),
        ];
        let max = info.iter().map(|(slot, size)| {
            *slot
        }).max().unwrap();
        let base = 1000;
        let info = info.into_iter().map(|(slot, size)| {
            (max - slot + base, size)
        }).collect::<Vec<_>>();
        calc(&info, normal_bin_widths());
        calc(&info, normal_ancient());
        calc(&info, normal_10k());
        return;
    }

    use std::fs;
    let dir = "/home/sol/ledger/accounts/run";
    let path = Path::new(dir);
    if path.is_dir() {
        let dir = fs::read_dir(&path);
        if let Ok(dir) = dir {
            let mut info = Vec::default();
            for entry in dir.flatten() {
                if let Some(name) = entry.path().file_name() {
                    let name = name.to_str().unwrap().split_once(".").unwrap().0;
                    let len = fs::metadata(entry.path()).unwrap().len();
                    info.push((name.parse::<usize>().unwrap(), len as usize));
                    // eprintln!("{name}, {len}");
                }
            }
            calc(&info, normal_bin_widths());
            calc(&info, normal_ancient());
            calc(&info, normal_10k());
            }
        else {
            panic!("couldn't read folder: {path:?}, {:?}", dir);
        }
    }
    else {
        panic!("not a folder: {}", dir);
    }
    return;

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("file")
                .index(1)
                .takes_value(true)
                .value_name("PATH")
                .help("Account storage file to open"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .takes_value(false)
                .help("Show additional account information"),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let file = value_t_or_exit!(matches, "file", String);
    let store = AppendVec::new_for_store_tool(&file).unwrap_or_else(|err| {
        eprintln!("failed to open storage file '{file}': {err}");
        std::process::exit(1);
    });
    // By default, when the AppendVec is dropped, the backing file will be removed.
    // We do not want to remove the backing file here in the store-tool, so prevent dropping.
    let store = ManuallyDrop::new(store);

    // max data size is 10 MiB (10,485,760 bytes)
    // therefore, the max width is ceil(log(10485760))
    let data_size_width = (MAX_PERMITTED_DATA_LENGTH as f64).log10().ceil() as usize;
    let offset_width = (store.capacity() as f64).log(16.0).ceil() as usize;

    let mut num_accounts = Saturating(0usize);
    let mut stored_accounts_size = Saturating(0);
    store.scan_accounts(|account| {
        if verbose {
            println!("{account:?}");
        } else {
            println!(
                "{:#0offset_width$x}: {:44}, owner: {:44}, data size: {:data_size_width$}, lamports: {}",
                account.offset(),
                account.pubkey().to_string(),
                account.owner().to_string(),
                account.data_len(),
                account.lamports(),
            );
        }
        num_accounts += 1;
        stored_accounts_size += account.stored_size();
    });

    println!(
        "number of accounts: {}, stored accounts size: {}, file size: {}",
        num_accounts,
        stored_accounts_size,
        store.capacity(),
    );
}
