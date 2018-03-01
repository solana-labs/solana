extern crate silk;

use silk::accountant_skel::AccountantSkel;
use silk::accountant::Accountant;
use silk::log::Sha256Hash;

fn main() {
    let addr = "127.0.0.1:8000";
    let zero = Sha256Hash::default();
    let acc = Accountant::new(&zero, Some(1000));
    let mut skel = AccountantSkel::new(acc);
    skel.serve(addr).unwrap();
}
