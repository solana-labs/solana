use bincode::serialize;
use drone::DroneRequest;
use signature::PublicKey;
use std::error;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};

pub fn request_airdrop(
    drone_addr: &SocketAddr,
    id: &PublicKey,
    tokens: u64,
) -> Result<(), Box<error::Error>> {
    let mut stream = TcpStream::connect(drone_addr)?;
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: tokens,
        client_pubkey: *id,
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx).unwrap();
    // TODO: add timeout to this function, in case of unresponsive drone
    Ok(())
}
