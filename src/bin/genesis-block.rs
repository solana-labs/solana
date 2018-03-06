//! A command-line executable for generating the chain's genesis block.

extern crate ring;
extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use silk::log::verify_slice_i64;
use std::io::stdin;

fn main() {
    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    let entries = gen.create_entries();
    verify_slice_i64(&entries, &entries[0].id);
    for x in entries {
        println!("{}", serde_json::to_string(&x).unwrap());
    }
}
