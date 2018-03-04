extern crate serde_json;
extern crate silk;

use silk::genesis::{Creator, Genesis};
use silk::event::{generate_keypair, get_pubkey};

fn main() {
    let alice = Creator {
        tokens: 200,
        pubkey: get_pubkey(&generate_keypair()),
    };
    let bob = Creator {
        tokens: 100,
        pubkey: get_pubkey(&generate_keypair()),
    };
    let creators = vec![alice, bob];
    let gen = Genesis::new(300, creators);
    println!("{}", serde_json::to_string(&gen).unwrap());
}
