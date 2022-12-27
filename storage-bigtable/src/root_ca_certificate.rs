use {
    std::{fs::File, io::Read},
    tonic::transport::Certificate,
};

pub fn load() -> Result<Certificate, String> {
    // Respect the standard GRPC_DEFAULT_SSL_ROOTS_FILE_PATH environment variable if present,
    // otherwise use the built-in root certificate
    let pem = match std::env::var("GRPC_DEFAULT_SSL_ROOTS_FILE_PATH").ok() {
        Some(cert_file) => File::open(&cert_file)
            .and_then(|mut file| {
                let mut pem = Vec::new();
                file.read_to_end(&mut pem).map(|_| pem)
            })
            .map_err(|err| format!("Failed to read {cert_file}: {err}"))?,
        None => {
            // PEM file from Google Trust Services (https://pki.goog/roots.pem)
            include_bytes!("pki-goog-roots.pem").to_vec()
        }
    };
    Ok(Certificate::from_pem(pem))
}
