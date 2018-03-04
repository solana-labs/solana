extern crate serde_json;
extern crate silk;

use silk::accountant_skel::AccountantSkel;
use silk::accountant::Accountant;
use silk::genesis::Genesis;
use std::io::stdin;

fn main() {
    let addr = "127.0.0.1:8000";
    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();
    let acc = Accountant::new(&gen, Some(1000));
    let mut skel = AccountantSkel::new(acc);
    println!("Listening on {}", addr);
    skel.serve(addr).unwrap();
}
