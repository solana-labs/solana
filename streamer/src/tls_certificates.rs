use {
    solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer},
    x509_parser::{prelude::*, public_key::PublicKey},
};

pub fn new_dummy_x509_certificate(keypair: &Keypair) -> (rustls::Certificate, rustls::PrivateKey) {
    // Unfortunately, rustls does not accept a "raw" Ed25519 key.
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

    // Create a dummy certificate. Only the SubjectPublicKeyInfo field
    // is relevant to the peer-to-peer protocols. The signature of the
    // X.509 certificate is deliberately invalid. (Peer authenticity is
    // checked in the TLS 1.3 CertificateVerify)
    // See https://www.itu.int/rec/T-REC-X.509-201910-I/en for detailed definitions.

    let mut cert_der = Vec::<u8>::with_capacity(0xf4);
    //    Certificate SEQUENCE (3 elem)
    //      tbsCertificate TBSCertificate SEQUENCE (8 elem)
    //        version [0] (1 elem)
    //          INTEGER  2
    //        serialNumber CertificateSerialNumber INTEGER (62 bit)
    //        signature AlgorithmIdentifier SEQUENCE (1 elem)
    //          algorithm OBJECT IDENTIFIER 1.3.101.112 curveEd25519 (EdDSA 25519 signature algorithm)
    //        issuer Name SEQUENCE (1 elem)
    //          RelativeDistinguishedName SET (1 elem)
    //            AttributeTypeAndValue SEQUENCE (2 elem)
    //              type AttributeType OBJECT IDENTIFIER 2.5.4.3 commonName (X.520 DN component)
    //              value AttributeValue [?] UTF8String Solana
    //        validity Validity SEQUENCE (2 elem)
    //          notBefore Time UTCTime 1970-01-01 00:00:00 UTC
    //          notAfter Time GeneralizedTime 4096-01-01 00:00:00 UTC
    //        subject Name SEQUENCE (0 elem)
    //        subjectPublicKeyInfo SubjectPublicKeyInfo SEQUENCE (2 elem)
    //          algorithm AlgorithmIdentifier SEQUENCE (1 elem)
    //            algorithm OBJECT IDENTIFIER 1.3.101.112 curveEd25519 (EdDSA 25519 signature algorithm)
    //          subjectPublicKey BIT STRING (256 bit)
    cert_der.extend_from_slice(&[
        0x30, 0x81, 0xf6, 0x30, 0x81, 0xa9, 0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x08, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x30, 0x16,
        0x31, 0x14, 0x30, 0x12, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x0b, 0x53, 0x6f, 0x6c, 0x61,
        0x6e, 0x61, 0x20, 0x6e, 0x6f, 0x64, 0x65, 0x30, 0x20, 0x17, 0x0d, 0x37, 0x30, 0x30, 0x31,
        0x30, 0x31, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x5a, 0x18, 0x0f, 0x34, 0x30, 0x39, 0x36,
        0x30, 0x31, 0x30, 0x31, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x5a, 0x30, 0x00, 0x30, 0x2a,
        0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ]);
    cert_der.extend_from_slice(&keypair.pubkey().to_bytes());
    //        extensions [3] (1 elem)
    //          Extensions SEQUENCE (2 elem)
    //            Extension SEQUENCE (3 elem)
    //              extnID OBJECT IDENTIFIER 2.5.29.17 subjectAltName (X.509 extension)
    //              critical BOOLEAN true
    //              extnValue OCTET STRING (13 byte) encapsulating
    //                SEQUENCE (1 elem)
    //                [2] (9 byte) localhost
    //            Extension SEQUENCE (3 elem)
    //              extnID OBJECT IDENTIFIER 2.5.29.19 basicConstraints (X.509 extension)
    //              critical BOOLEAN true
    //              extnValue OCTET STRING (2 byte) encapsulating
    //                SEQUENCE (0 elem)
    //      signatureAlgorithm AlgorithmIdentifier SEQUENCE (1 elem)
    //        algorithm OBJECT IDENTIFIER 1.3.101.112 curveEd25519 (EdDSA 25519 signature algorithm)
    //        signature BIT STRING (512 bit)
    cert_der.extend_from_slice(&[
        0xa3, 0x29, 0x30, 0x27, 0x30, 0x17, 0x06, 0x03, 0x55, 0x1d, 0x11, 0x01, 0x01, 0xff, 0x04,
        0x0d, 0x30, 0x0b, 0x82, 0x09, 0x6c, 0x6f, 0x63, 0x61, 0x6c, 0x68, 0x6f, 0x73, 0x74, 0x30,
        0x0c, 0x06, 0x03, 0x55, 0x1d, 0x13, 0x01, 0x01, 0xff, 0x04, 0x02, 0x30, 0x00, 0x30, 0x05,
        0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x41, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ]);

    (
        rustls::Certificate(cert_der),
        rustls::PrivateKey(key_pkcs8_der),
    )
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
    use {super::*, solana_sdk::signer::Signer};

    #[test]
    fn test_generate_tls_certificate() {
        let keypair = Keypair::new();
        let (cert, _) = new_dummy_x509_certificate(&keypair);
        if let Some(pubkey) = get_pubkey_from_tls_certificate(&cert) {
            assert_eq!(pubkey, keypair.pubkey());
        } else {
            panic!("Failed to get certificate pubkey");
        }
    }
}
