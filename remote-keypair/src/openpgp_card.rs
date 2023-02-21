use {
    openpgp_card::{
        algorithm::{Algo, Curve},
        crypto_data::{EccType, PublicKeyMaterial, Hash},
        OpenPgp,
        SmartcardError,
        card_do::{UIF, ApplicationIdentifier},
        OpenPgpTransaction
    },
    openpgp_card_pcsc::PcscBackend,
    pinentry::PassphraseInput,
    secrecy::ExposeSecret,
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Signature, Signer, SignerError},
    },
    std::cell::RefCell,
    thiserror::Error,
    uriparse::{URIReference, URIReferenceError},
};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum LocatorError {
    #[error("failed to parse OpenPGP AID: {0}")]
    IdentifierParseError(String),
    #[error(transparent)]
    UriReferenceError(#[from] URIReferenceError),
    #[error("mismatched scheme")]
    MismatchedScheme,
}

/// To locate smart cards that support OpenPGP connected to the local machine.
/// 
/// Field `aid` contains a data struct with fields for application ID, version,
/// manufacturer ID (of the smart card), and serial number. An instance of
/// `ApplicationIdentifier` is logically equivalent to an AID string as
/// specified in the OpenPGP specification v3.4 (section 4.2.1), used to
/// uniquely identify an instance of an OpenPGP application on a unique smart
/// card.
/// 
/// A null `aid` indicates the default locator which chooses the first smart
/// card supporting OpenPGP that it finds connected to the machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Locator {
    pub aid: Option<ApplicationIdentifier>,
}

impl Locator {
    /// Extract a locator from a URI.
    /// 
    /// "pgpcard://" => Default locator.
    /// 
    /// "pgpcard://D2760001240103040006123456780000" => Locator pointing to a
    /// OpenPGP instance identifiable by AID D2760001240103040006123456780000.
    /// As per the OpenPGP specification:
    ///   * AID must be a 16-byte hexadecimal string (32 digits).
    ///   * D276000124 => AID preamble, fixed across all AIDs
    ///   * 01         => application ID, fixed to 01 == OpenPGP
    ///   * 0304       => OpenPGP version, 3.4 in this case
    ///   * 0006       => unique manufacturer ID, Yubico in this case
    ///   * 12345678   => smart card serial number
    ///   * 0000       => reserved for future use
    /// 
    /// No other URI formats are valid.
    pub fn new_from_uri(uri: &URIReference<'_>) -> Result<Self, LocatorError> {
        let scheme = uri.scheme().map(|s| s.as_str().to_ascii_lowercase());
        let ident = uri.host().map(|h| h.to_string());
        
        match (scheme, ident) {
            (Some(scheme), Some(ident)) if scheme == "pgpcard" => {
                if ident.is_empty() {
                    return Ok(Self {
                        aid: None,
                    });
                }

                if ident.len() % 2 != 0 {
                    return Err(LocatorError::IdentifierParseError("OpenPGP AID must have even length".to_string()));
                }
                
                let mut ident_bytes = Vec::<u8>::new();
                for i in (0..ident.len()).step_by(2) {
                    ident_bytes.push(
                        u8::from_str_radix(&ident[i..i + 2], 16)
                            .map_err(|_| LocatorError::IdentifierParseError(
                                "non-hex character found in identifier".to_string()
                            ))?
                    );
                }

                Ok(Self {
                    aid: Some(
                        ident_bytes
                            .as_slice()
                            .try_into()
                            .map_err(|_| LocatorError::IdentifierParseError(
                                "invalid identifier format".to_string()
                            ))?
                    ),
                })
            },
            (Some(scheme), None) if scheme == "pgpcard" => {
                Ok(Self {
                    aid: None,
                })
            },
            _ => Err(LocatorError::MismatchedScheme),
        }
    }
}

/// Data struct for convenience.
#[derive(Debug)]
#[allow(dead_code)]  // unused pcsc_identifier
pub struct OpenpgpCardInfo {
    pcsc_identifier: String,
    manufacturer: String,
    serial: u32,
    cardholder_name: String,
    signing_uif: Option<UIF>,
    pin_cds_valid_once: bool,
}

impl TryFrom<&mut OpenPgpTransaction<'_>> for OpenpgpCardInfo {
    type Error = openpgp_card::Error;

    fn try_from(opt: &mut OpenPgpTransaction) -> Result<Self, Self::Error> {
        let ard = opt.application_related_data()?;
        let card_app_info = ard.application_id()?;
        let cardholder_info = opt.cardholder_related_data()?;
        Ok(OpenpgpCardInfo {
            pcsc_identifier: ard.application_id()?.ident(),
            manufacturer: card_app_info.manufacturer_name().to_string(),
            serial: ard.application_id()?.serial(),
            cardholder_name: String::from_utf8_lossy(cardholder_info.name().get_or_insert(b"null")).to_string(),
            signing_uif: ard.uif_pso_cds()?,
            pin_cds_valid_once: ard.pw_status_bytes()?.pw1_cds_valid_once(),
        })
    }
}

impl OpenpgpCardInfo {
    pub fn find_cards() -> Result<Vec<OpenpgpCardInfo>, openpgp_card::Error> {
        let mut card_infos = Vec::<OpenpgpCardInfo>::new();
        for backend in PcscBackend::cards(None)? {
            let mut pgp = OpenPgp::new::<PcscBackend>(backend.into());
            let opt = &mut pgp.transaction()?;
            card_infos.push(opt.try_into()?);
        }
        Ok(card_infos)
    }
}

pub struct OpenpgpCardKeypair {
    pgp: RefCell<OpenPgp>,
    pubkey: Pubkey,
    pin_verified: RefCell<bool>,
}

impl OpenpgpCardKeypair {
    pub fn new_from_locator(
        locator: Locator,
    ) -> Result<Self, openpgp_card::Error> {
        Ok(Self::new_from_identifier(
            locator.aid.map(|x| x.ident())
        )?)
    }

    /// Create new OpenpgpCardKeypair from a openpgp-card library "ident".
    /// 
    /// The openpgp-card library uses shorthand AIDs to identify OpenPGP apps.
    /// An ident takes the form `<manufacturer ID>:<serial number>`, so the
    /// ident for AID `D2760001240103040006123456780000` would be
    /// `0006:12345678`.
    /// 
    /// As per the specification, a single smart card should only run one instance
    /// of the OpenPGP application, so uniquely identifying a smart card is
    /// sufficient to uniquely identify a OpenPGP instance.
    pub fn new_from_identifier(
        pcsc_identifier: Option<String>,
    ) -> Result<Self, openpgp_card::Error> {
        // Initialize long-lived PCSC backend object to interface with OpenPGP
        // add on the card.
        let backend = match pcsc_identifier {
            Some(ident) => PcscBackend::open_by_ident(&ident, None)?,
            None => {
                let mut cards = PcscBackend::cards(None)?;
                if cards.is_empty() {
                    return Err(openpgp_card::Error::Smartcard(SmartcardError::NoReaderFoundError))
                } else {
                    cards.remove(0)
                }
            }
        };
        let mut pgp = OpenPgp::new::<PcscBackend>(backend.into());
        
        let pubkey: [u8; 32];
        {
            // To get pubkey (or to do any operation with the OpenPGP app on
            // card), initialize a short-lived OpenPGP transaction object.
            let opt = &mut pgp.transaction()?;
            // Get smart card's PGP signing key.
            let pk_material = match opt.public_key(openpgp_card::KeyType::Signing) {
                Ok(pkm) => pkm,
                Err(_) => return Err(
                    openpgp_card::Error::NotFound("no valid key found on card".to_string())
                )
            };

            // Verify signing key is an ed25519 key using EdDSA and extract
            // the pubkey as bytes.
            pubkey = match pk_material {
                PublicKeyMaterial::E(pk) => match pk.algo() {
                    Algo::Ecc(ecc_attrs) => {
                        if ecc_attrs.ecc_type() != EccType::EdDSA || ecc_attrs.curve() != Curve::Ed25519 {
                            return Err(openpgp_card::Error::UnsupportedAlgo(
                                format!("expected Ed25519 key, got {:?}", ecc_attrs.curve())
                            ));
                        }
                        match pk.data().try_into() {
                            Ok(pk_bytes) => pk_bytes,
                            Err(_) => return Err(
                                openpgp_card::Error::UnsupportedAlgo("invalid pubkey format".to_string())
                            ),
                        }
                    },
                    _ => return Err(
                        openpgp_card::Error::UnsupportedAlgo("expected ECC key, got RSA".to_string())
                    ),
                }
                _ => return Err(
                    openpgp_card::Error::UnsupportedAlgo("expected ECC key, got RSA".to_string())
                ),
            };
        }

        Ok(Self {
            pgp: RefCell::new(pgp),
            pubkey: Pubkey::from(pubkey),
            pin_verified: RefCell::new(false),
        })
    }
}

impl Signer for OpenpgpCardKeypair {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        Ok(self.pubkey)
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        let mut pgp_mut = self.pgp.borrow_mut();
        let opt = &mut match pgp_mut.transaction() {
            Ok(opt) => opt,
            Err(e) => return Err(
                SignerError::Connection(format!("could not start transaction with card: {}", e))
            )
        };

        let card_info: OpenpgpCardInfo = match opt.try_into() {
            Ok(info) => info,
            Err(e) => return Err(
                SignerError::Connection(format!("could not get card info: {}", e))
            )
        };

        // Prompt user for PIN verification if and only if
        //   * Card indicates PIN is only valid for one PSO:CDS command at a time, or
        //   * PIN has not yet been entered for the first time.
        if card_info.pin_cds_valid_once || !*self.pin_verified.borrow() {
            let description = format!(
                "\
                    Please unlock the card%0A\
                    %0A\
                    Manufacturer: {}%0A\
                    Serial: {:X}%0A\
                    Cardholder: {}\
                ",
                card_info.manufacturer,
                card_info.serial,
                card_info.cardholder_name,
            );
            let pin = 
                if let Some(mut input) = PassphraseInput::with_default_binary() {
                    input
                        .with_description(description.as_str())
                        .with_prompt("PIN")
                        .interact()
                } else {
                    return Err(
                        SignerError::Custom("pinentry binary not found, please install".to_string())
                    )
                };
            let pin = match pin {
                Ok(secret) => secret,
                Err(e) => return Err(
                    SignerError::InvalidInput(format!("cannot read PIN from user: {}", e))
                ),
            };
            match opt.verify_pw1_sign(pin.expose_secret().as_bytes()) {
                Ok(_) => {
                    *self.pin_verified.borrow_mut() = true;
                },
                Err(e) => return Err(
                    SignerError::InvalidInput(format!("invalid PIN: {}", e))
                ),
            }
        }

        // Await user touch confirmation if and only if
        //   * Card supports touch confirmation, and
        //   * Touch policy set anything other than "off".
        if let Some(signing_uif) = card_info.signing_uif {
            if signing_uif.touch_policy().touch_required() {
                println!("Awaiting touch confirmation...");
            }
        }

        // Delegate message signing to card
        let hash = Hash::EdDSA(message);
        let sig = match opt.signature_for_hash(hash) {
            Ok(sig) => sig,
            Err(e) => return Err(
                SignerError::Protocol(format!("card failed to sign message: {}", e))
            ),
        };

        Ok(Signature::new(&sig[..]))
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_locator() {
        // no identifier in URI => default locator
        let uri = URIReference::try_from("pgpcard://").unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Ok(Locator { aid: None }),
        );

        // valid identifier in URI
        let uri = URIReference::try_from("pgpcard://D2760001240103040006123456780000").unwrap();
        let expected_ident_bytes: [u8; 16] = [
            0xD2, 0x76, 0x00, 0x01, 0x24,   // preamble
            0x01,                           // application id (OpenPGP)
            0x03, 0x04,                     // version
            0x00, 0x06,                     // manufacturer id
            0x12, 0x34, 0x56, 0x78,         // serial number
            0x00, 0x00                      // reserved
        ];
        assert_eq!(
            Locator::new_from_uri(&uri),
            Ok(Locator { aid: Some(ApplicationIdentifier::try_from(&expected_ident_bytes[..]).unwrap()) }),
        );

        // non-hex character in identifier
        let uri = URIReference::try_from("pgpcard://G2760001240103040006123456780000").unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::IdentifierParseError("non-hex character found in identifier".to_string())),
        );

        // invalid identifier length
        let uri = URIReference::try_from("pgpcard://D27600012401030400061234567800").unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::IdentifierParseError("invalid identifier format".to_string())),
        );
    }
}