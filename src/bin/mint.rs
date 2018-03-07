extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use std::io;

fn main() {
    let mut input_text = String::new();
    io::stdin().read_line(&mut input_text).unwrap();

    let trimmed = input_text.trim();
    let tokens = trimmed.parse::<i64>().unwrap();
    let gen = Genesis::new(tokens);
    println!("{}", serde_json::to_string(&gen).unwrap());
}
