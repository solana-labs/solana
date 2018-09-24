extern crate influx_db_client;
extern crate serde_json;
extern crate solana;
use influx_db_client as influxdb;
use serde_json::Value;
use solana::metrics;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().collect();
    // Open the path in read-only mode, returns `io::Result<File>`
    let fname = &args[1];
    let file = match File::open(fname) {
        Err(why) => panic!("couldn't open {}: {:?}", fname, why),
        Ok(file) => file,
    };

    let git_output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("failed to execute git rev-parse");
    let git_commit_hash = String::from_utf8_lossy(&git_output.stdout);
    let trimmed_hash = git_commit_hash.trim().to_string();
    println!("uploading hash: {}", trimmed_hash);

    for line in BufReader::new(file).lines() {
        if let Ok(v) = serde_json::from_str(&line.unwrap()) {
            let v: Value = v;
            if v["type"] == "bench" {
                println!("{}", v);
                println!("  {}", v["type"]);
                let median = v["median"].to_string().parse().unwrap();
                let deviation = v["deviation"].to_string().parse().unwrap();
                metrics::submit(
                    influxdb::Point::new(&v["name"].as_str().unwrap().trim_matches('\"'))
                        .add_field("median", influxdb::Value::Integer(median))
                        .add_field("deviation", influxdb::Value::Integer(deviation))
                        .add_field(
                            "commit",
                            influxdb::Value::String(git_commit_hash.trim().to_string()),
                        ).to_owned(),
                );
            }
        }
    }
    metrics::flush();
}
