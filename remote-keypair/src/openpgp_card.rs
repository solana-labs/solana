use {
    openpgp_card::{
        crypto_data::{PublicKeyMaterial, Hash},
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
    std::{cell::RefCell, convert::Infallible},
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
    #[error("infallible")]
    Infallible,
}

impl From<Infallible> for LocatorError {
    fn from(_: Infallible) -> Self {
        Self::Infallible
    }
}

pub struct Locator {
    aid: Option<ApplicationIdentifier>,
}

impl Locator {
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
                    return Err(LocatorError::IdentifierParseError(String::from("OpenPGP AID must have even length")));
                }
                
                let mut ident_bytes = Vec::<u8>::new();
                for i in (0..ident.len()).step_by(2) {
                    ident_bytes.push(
                        u8::from_str_radix(&ident[i..i + 2], 16)
                            .map_err(|e| LocatorError::IdentifierParseError(e.to_string()))?
                    );
                }

                Ok(Self {
                    aid: Some(
                        ident_bytes
                            .as_slice()
                            .try_into()
                            .map_err(|e: openpgp_card::Error| LocatorError::IdentifierParseError(e.to_string()))?
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

#[derive(Debug)]
#[allow(dead_code)]
pub struct OpenpgpCardInfo {
    pcsc_identifier: String,
    manufacturer: String,
    serial: u32,
    cardholder_name: String,
    signing_uif: Option<UIF>,
}

impl TryFrom<&mut OpenPgpTransaction<'_>> for OpenpgpCardInfo {
    type Error = openpgp_card::Error;

    fn try_from(opt: &mut OpenPgpTransaction) -> Result<Self, Self::Error> {
        let ard = opt.application_related_data()?;
        let card_app_info = ard.application_id()?;
        let cardholder_info = opt.cardholder_related_data()?;
        Ok(OpenpgpCardInfo {
            pcsc_identifier: ard.application_id()?.ident(),
            manufacturer: String::from(card_app_info.manufacturer_name()),
            serial: ard.application_id()?.serial(),
            cardholder_name: String::from_utf8_lossy(cardholder_info.name().get_or_insert(b"null")).to_string(),
            signing_uif: ard.uif_pso_cds()?,
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
}

impl OpenpgpCardKeypair {
    pub fn new_from_locator(
        locator: Locator,
    ) -> Result<Self, openpgp_card::Error> {
        Ok(Self::new_from_identifier(
            locator.aid.map(|x| x.ident())
        )?)
    }

    pub fn new_from_identifier(
        pcsc_identifier: Option<String>,
    ) -> Result<Self, openpgp_card::Error> {
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
            let opt = &mut pgp.transaction()?;
            let pk_material = match opt.public_key(openpgp_card::KeyType::Signing) {
                Ok(pkm) => pkm,
                Err(_) => return Err(
                    openpgp_card::Error::NotFound(String::from("no valid key found on card"))
                )
            };

            // TODO: check that key is ed25519 and not just ECC
            pubkey = match pk_material {
                PublicKeyMaterial::E(pk) => match pk.data().try_into() {
                    Ok(pk_bytes) => pk_bytes,
                    Err(_) => return Err(
                        openpgp_card::Error::UnsupportedAlgo(String::from("invalid pubkey format"))
                    ),
                },
                _ => return Err(
                    openpgp_card::Error::UnsupportedAlgo(String::from("expected ECC key, got RSA"))
                ),
            };
        }

        Ok(Self {
            pgp: RefCell::new(pgp),
            pubkey: Pubkey::from(pubkey),
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

        // Prompt user for PIN verification
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
                    SignerError::Custom(String::from("pinentry binary not found, please install"))
                )
            };
        let pin = match pin {
            Ok(secret) => secret,
            Err(e) => return Err(
                SignerError::InvalidInput(format!("cannot read PIN from user: {}", e))
            ),
        };
        match opt.verify_pw1_sign(pin.expose_secret().as_bytes()) {
            Err(e) => return Err(
                SignerError::InvalidInput(format!("invalid PIN: {}", e))
            ),
            _ => (),
        }

        // Await user touch confirmation if and only if card supports touch
        // and touch policy set anything other than "off"
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
        println!("Message signed");

        Ok(Signature::new(&sig[..]))
    }

    fn is_interactive(&self) -> bool {
        true
    }
}
