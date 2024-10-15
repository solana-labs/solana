#[cfg(feature = "full")]
pub use solana_pubkey::new_rand;
#[cfg(target_os = "solana")]
pub use solana_pubkey::syscalls;
pub use solana_pubkey::{
    bytes_are_curve_point, ParsePubkeyError, Pubkey, PubkeyError, MAX_SEEDS, MAX_SEED_LEN,
    PUBKEY_BYTES,
};

#[deprecated(since = "2.1.0")]
#[cfg(feature = "full")]
pub fn write_pubkey_file(outfile: &str, pubkey: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let printable = format!("{pubkey}");
    let serialized = serde_json::to_string(&printable)?;

    if let Some(outdir) = std::path::Path::new(&outfile).parent() {
        std::fs::create_dir_all(outdir)?;
    }
    let mut f = std::fs::File::create(outfile)?;
    f.write_all(&serialized.into_bytes())?;

    Ok(())
}

#[deprecated(since = "2.1.0")]
#[cfg(feature = "full")]
pub fn read_pubkey_file(infile: &str) -> Result<Pubkey, Box<dyn std::error::Error>> {
    let f = std::fs::File::open(infile)?;
    let printable: String = serde_json::from_reader(f)?;

    use std::str::FromStr;
    Ok(Pubkey::from_str(&printable)?)
}
