//! A command-line executable for generating the chain's genesis block.

extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use std::io::stdin;

fn main() {
    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    for x in gen.create_entries() {
        println!("{}", serde_json::to_string(&x).unwrap());
    }
}
