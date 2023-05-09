use {
    pkcs8::{der::Document, AlgorithmIdentifier, ObjectIdentifier},
    rcgen::{CertificateParams, DistinguishedName, DnType, RcgenError, SanType},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::net::IpAddr,
    x509_parser::{prelude::*, public_key::PublicKey},
};

pub fn new_self_signed_tls_certificate(
    keypair: &Keypair,
    san: IpAddr,
) -> Result<(rustls::Certificate, rustls::PrivateKey), RcgenError> {
    // TODO(terorie): Is it safe to sign the TLS cert with the identity private key?

    // Unfortunately, rcgen does not accept a "raw" Ed25519 key.
    // We have to convert it to DER and pass it to the library.

    // Convert private key into PKCS#8 v1 object.
    // RFC 8410, Section 7: Private Key Format
    // https://datatracker.ietf.org/doc/html/rfc8410#section-

    // from https://datatracker.ietf.org/doc/html/rfc8410#section-3
    const ED25519_IDENTIFIER: [u32; 4] = [1, 3, 101, 112];
    let mut private_key = Vec::<u8>::with_capacity(34);
    private_key.extend_from_slice(&[0x04, 0x20]); // ASN.1 OCTET STRING
    private_key.extend_from_slice(keypair.secret().as_bytes());
    let key_pkcs8 = pkcs8::PrivateKeyInfo {
        algorithm: AlgorithmIdentifier {
            oid: ObjectIdentifier::from_arcs(&ED25519_IDENTIFIER).expect("Failed to convert OID"),
            parameters: None,
        },
        private_key: &private_key,
        public_key: None,
    };
    let key_pkcs8_der = key_pkcs8
        .to_der()
        .expect("Failed to convert keypair to DER")
        .to_der();

    let rcgen_keypair = rcgen::KeyPair::from_der(&key_pkcs8_der)?;

    let mut cert_params = CertificateParams::default();
    cert_params.subject_alt_names = vec![SanType::IpAddress(san)];
    cert_params.alg = &rcgen::PKCS_ED25519;
    cert_params.key_pair = Some(rcgen_keypair);
    cert_params.distinguished_name = DistinguishedName::new();
    cert_params
        .distinguished_name
        .push(DnType::CommonName, "Solana node");

    let cert = rcgen::Certificate::from_params(cert_params)?;
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    Ok((rustls::Certificate(cert_der), priv_key))
}

pub fn get_pubkey_from_tls_certificate(der_cert: &rustls::Certificate) -> Option<Pubkey> {
    let (_, cert) = X509Certificate::from_der(der_cert.as_ref()).ok()?;
    match cert.public_key().parsed().ok()? {
        PublicKey::Unknown(key) => Pubkey::try_from(key).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::signer::Signer, std::net::Ipv4Addr};

    #[test]
    fn test_generate_tls_certificate() {
        let keypair = Keypair::new();

        if let Ok((cert, _)) =
            new_self_signed_tls_certificate(&keypair, IpAddr::V4(Ipv4Addr::LOCALHOST))
        {
            if let Some(pubkey) = get_pubkey_from_tls_certificate(&cert) {
                assert_eq!(pubkey, keypair.pubkey());
            } else {
                panic!("Failed to get certificate pubkey");
            }
        } else {
            panic!("Failed to generate certificates");
        }
    }
}
