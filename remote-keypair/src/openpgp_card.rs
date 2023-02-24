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

/// Locator for smart cards supporting OpenPGP connected to the local machine.
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

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum LocatorError {
    #[error("failed to parse OpenPGP AID: {0}")]
    IdentifierParseError(String),
    #[error(transparent)]
    UriReferenceError(#[from] URIReferenceError),
    #[error("mismatched scheme")]
    MismatchedScheme,
}

impl TryFrom<&URIReference<'_>> for Locator {
    type Error = LocatorError;

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
    fn try_from(uri: &URIReference<'_>) -> Result<Self, Self::Error> {
        let scheme = uri.scheme().map(|s| s.as_str().to_ascii_lowercase());
        let ident = uri.host().map(|h| h.to_string());
        
        match (scheme, ident) {
            (Some(scheme), Some(ident)) if scheme == "pgpcard" => {
                if ident.is_empty() {
                    return Ok(Self { aid: None });
                }
                if ident.len() % 2 != 0 {
                    return Err(LocatorError::IdentifierParseError("OpenPGP AID must have even length".to_string()));
                }
                let mut ident_bytes = Vec::<u8>::new();
                for i in (0..ident.len()).step_by(2) {
                    ident_bytes.push(u8::from_str_radix(&ident[i..i + 2], 16).map_err(
                        |_| LocatorError::IdentifierParseError("non-hex character found in identifier".to_string())
                    )?);
                }
                Ok(Self {
                    aid: Some(ident_bytes.as_slice().try_into().map_err(
                        |_| LocatorError::IdentifierParseError("invalid identifier format".to_string())
                    )?),
                })
            },
            (Some(scheme), None) if scheme == "pgpcard" => Ok(Self { aid: None }),
            _ => Err(LocatorError::MismatchedScheme),
        }
    }
}

/// An ongoing connection to the OpenPGP application on a smart card.
/// 
/// Field `pgp: OpenPgp` is a long-lived card access object. When OpenpgpCard
/// is initialized via TryFrom<&Locator>, this object contains a `PcscBackend`
/// connector object that communicates with the card using PC/SC.
/// 
/// Actual operations on the card are done through short-lived 
/// OpenPgpTransaction objects that are instantiated from `OpenPgp` on an ad
/// hoc basis. There should only exist one active OpenPgpTransaction at a time.
/// 
/// Field `pin_verified` indicates whether a successful PIN verification has
/// already happened in the past (specifically for PW1 used to authorize
/// signing). This allows redundant PIN verification to be skipped for cards
/// that require only a single successful PIN entry per session.
pub struct OpenpgpCard {
    pgp: RefCell<OpenPgp>,
    pin_verified: RefCell<bool>,
}

impl TryFrom<&Locator> for OpenpgpCard {
    type Error = openpgp_card::Error;

    fn try_from(locator: &Locator) -> Result<Self, Self::Error> {
        let pcsc_identifier = locator.aid.as_ref().map(|x| x.ident());
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

        // Start up the long-lived OpenPgp backend connection object which will
        // be persisted as a field in the `OpenpgpCard` struct.
        let pgp = OpenPgp::new::<PcscBackend>(backend.into());

        Ok(Self {
            pgp: RefCell::new(pgp),
            pin_verified: RefCell::new(false),
        })
    }
}

impl Signer for OpenpgpCard {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        let mut pgp_mut = self.pgp.borrow_mut();
        let opt = &mut pgp_mut.transaction().map_err(
            |e| SignerError::Connection(format!("could not start transaction with card: {}", e))
        )?;

        // Verify smart card's PGP signing key is an ed25519 key using EdDSA
        // and extract the pubkey as bytes.
        let pk_material = opt.public_key(openpgp_card::KeyType::Signing).map_err(
            |e| SignerError::Connection(format!("could not find signing keypair on card: {}", e))
        )?;
        let pk_bytes: [u8; 32] = match pk_material {
            PublicKeyMaterial::E(pk) => match pk.algo() {
                Algo::Ecc(ecc_attrs) => {
                    if ecc_attrs.ecc_type() != EccType::EdDSA || ecc_attrs.curve() != Curve::Ed25519 {
                        return Err(SignerError::Connection(
                            format!("expected Ed25519 key, got {:?}", ecc_attrs.curve())
                        ));
                    }
                    pk.data().try_into().map_err(
                        |e| SignerError::Connection(format!("key on card is malformed: {}", e))
                    )?
                },
                _ => return Err(SignerError::Connection("expected ECC key, got RSA".to_string())),
            }
            _ => return Err(SignerError::Connection("expected ECC key, got RSA".to_string())),
        };

        Ok(Pubkey::from(pk_bytes))
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        // Verify that card has valid signing key
        self.try_pubkey()?;

        let mut pgp_mut = self.pgp.borrow_mut();
        let opt = &mut pgp_mut.transaction().map_err(
            |e| SignerError::Connection(format!("could not start transaction with card: {}", e))
        )?;
        let card_info: OpenpgpCardInfo = opt.try_into().map_err(
            |e| SignerError::Connection(format!("could not get card info: {}", e))
        )?;

        // Prompt user for PIN verification if and only if
        //   * Card indicates PIN is only valid for one PSO:CDS command at a time, or
        //   * PIN has not yet been entered for the first time.
        if card_info.pin_cds_valid_once || !*self.pin_verified.borrow() {
            let mut pin = get_pin_from_user_as_bytes(&card_info, true)?;
            while opt.verify_pw1_sign(pin.as_bytes()).is_err() {
                pin = get_pin_from_user_as_bytes(&card_info, false)?;
            }
            *self.pin_verified.borrow_mut() = true;
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
        let sig = opt.signature_for_hash(hash).map_err(
            |e| SignerError::Protocol(format!("card failed to sign message: {}", e))
        )?;

        Ok(Signature::new(&sig[..]))
    }

    fn is_interactive(&self) -> bool {
        true
    }
}

/// Data struct for convenience.
#[derive(Debug)]
struct OpenpgpCardInfo {
    aid: ApplicationIdentifier,
    cardholder_name: String,
    signing_uif: Option<UIF>,
    pin_cds_valid_once: bool,
}

impl TryFrom<&mut OpenPgpTransaction<'_>> for OpenpgpCardInfo {
    type Error = openpgp_card::Error;

    fn try_from(opt: &mut OpenPgpTransaction) -> Result<Self, Self::Error> {
        let ard = opt.application_related_data()?;
        Ok(OpenpgpCardInfo {
            aid: ard.application_id()?,
            cardholder_name: String::from_utf8_lossy(
                opt.cardholder_related_data()?.name().get_or_insert(b"null")
            ).to_string(),
            signing_uif: ard.uif_pso_cds()?,
            pin_cds_valid_once: ard.pw_status_bytes()?.pw1_cds_valid_once(),
        })
    }
}

fn get_pin_from_user_as_bytes(card_info: &OpenpgpCardInfo, first_attempt: bool) -> Result<String, SignerError> {
    let description = format!(
        "\
            Please unlock the card%0A\
            %0A\
            Manufacturer: {}%0A\
            Serial: {:X}%0A\
            Cardholder: {}\
            {}\
        ",
        card_info.aid.manufacturer_name(),
        card_info.aid.serial(),
        card_info.cardholder_name,
        if first_attempt { "" } else { "%0A%0A##### INVALID PIN #####" },
    );
    let pin = if let Some(mut input) = PassphraseInput::with_default_binary() {
        input.with_description(description.as_str()).with_prompt("PIN").interact().map_err(
            |e| SignerError::InvalidInput(format!("cannot read PIN from user: {}", e))
        )?
    } else {
        return Err(SignerError::Custom("pinentry binary not found, please install".to_string()));
    };
    Ok(pin.expose_secret().to_owned())
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
