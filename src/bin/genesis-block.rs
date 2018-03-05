//! A command-line executable for generating the chain's genesis block.

extern crate ring;
extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use silk::log::verify_slice_u64;
use std::io::stdin;

fn main() {
    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    let entries = gen.create_entries();
    verify_slice_u64(&entries, &entries[0].id);
    println!("[");
    let len = entries.len();
    for (i, x) in entries.iter().enumerate() {
        let s = serde_json::to_string(&x).unwrap();

        let terminator = if i + 1 == len { "" } else { "," };
        println!("    {}{}", s, terminator);
    }
    println!("]");
}
