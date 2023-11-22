use {
    rcgen::{CertificateParams, DistinguishedName, DnType, RcgenError, SanType},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::net::IpAddr,
    x509_parser::{prelude::*, public_key::PublicKey},
};

pub fn new_self_signed_tls_certificate(
    keypair: &Keypair,
    san: IpAddr,
) -> Result<(rustls::Certificate, rustls::PrivateKey), RcgenError> {
    // Note: This function signs an X.509 certificate with the node
    // identity key, which is theoretically redundant. (Peer validation
    // is done at a later step in the TLS CertificateVerify) We are
    // currently forced to use X.509 certificates regardless because the
    // Rust ecosystem's poor support for RFC 7250 (raw public keys).
    //
    // This form of key reuse introduces risk of type confusion attacks
    // against the signing payload. It was proven using a CBMC model
    // that no such type confusion attack exists as of 2023-Nov:
    //
    // https://github.com/firedancer-io/firedancer/blob/7e91d9bda93dee2a43065e30f65ef6422bd93019/src/disco/keyguard/fd_keyguard_match.c

    // TODO: Consider (1) signing with a dummy cert authority instead of
    //       using a self-signed certificate, or (2) forcing use of
    //       RFC 7250 RawPublicKeys.

    // Unfortunately, rcgen does not accept a "raw" Ed25519 key.
    // We have to convert it to DER and pass it to the library.

    // Convert private key into PKCS#8 v1 object.
    // RFC 8410, Section 7: Private Key Format
    // https://www.rfc-editor.org/rfc/rfc8410#section-7
    //
    // The hardcoded prefix decodes to the following ASN.1 structure:
    //
    //   PrivateKeyInfo SEQUENCE (3 elem)
    //     version Version INTEGER 0
    //     privateKeyAlgorithm AlgorithmIdentifier SEQUENCE (1 elem)
    //       algorithm OBJECT IDENTIFIER 1.3.101.112 curveEd25519 (EdDSA 25519 signature algorithm)
    //     privateKey PrivateKey OCTET STRING (34 byte)
    const PKCS8_PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    let mut key_pkcs8_der = Vec::<u8>::with_capacity(PKCS8_PREFIX.len() + 32);
    key_pkcs8_der.extend_from_slice(&PKCS8_PREFIX);
    key_pkcs8_der.extend_from_slice(keypair.secret().as_bytes());

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
