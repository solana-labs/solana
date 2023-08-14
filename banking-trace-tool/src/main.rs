use {clap::Parser, std::path::PathBuf};

#[derive(Parser)]
struct Args {
    /// The path to the banking trace event files.
    #[clap(short, long)]
    path: PathBuf,
}

fn main() {
    let Args { path } = Args::parse();

    for index in 0.. {
        let event_filename = if index == 0 {
            "events".to_owned()
        } else {
            format!("events.{index}")
        };
        let event_file = path.join(event_filename);
        if !event_file.exists() {
            break;
        }
        println!("Reading events from {}:", event_file.display());
    }
}
