use {
    crate::{
        input_parsers::STDOUT_OUTFILE_TOKEN,
        keypair::{
            keypair_from_source, pubkey_from_source, resolve_signer_from_source,
            signer_from_source, ASK_KEYWORD,
        },
    },
    clap::{builder::ValueParser, ArgMatches},
    solana_remote_wallet::{
        locator::{Locator as RemoteWalletLocator, LocatorError as RemoteWalletLocatorError},
        remote_wallet::RemoteWalletManager,
    },
    solana_sdk::{
        derivation_path::{DerivationPath, DerivationPathError},
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
    },
    std::{error, rc::Rc, str::FromStr},
    thiserror::Error,
};

const SIGNER_SOURCE_PROMPT: &str = "prompt";
const SIGNER_SOURCE_FILEPATH: &str = "file";
const SIGNER_SOURCE_USB: &str = "usb";
const SIGNER_SOURCE_STDIN: &str = "stdin";
const SIGNER_SOURCE_PUBKEY: &str = "pubkey";

#[derive(Debug, Error)]
pub enum SignerSourceError {
    #[error("unrecognized signer source")]
    UnrecognizedSource,
    #[error(transparent)]
    RemoteWalletLocatorError(#[from] RemoteWalletLocatorError),
    #[error(transparent)]
    DerivationPathError(#[from] DerivationPathError),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("unsupported source")]
    UnsupportedSource,
}

#[derive(Clone)]
pub enum SignerSourceKind {
    Prompt,
    Filepath(String),
    Usb(RemoteWalletLocator),
    Stdin,
    Pubkey(Pubkey),
}

impl AsRef<str> for SignerSourceKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::Prompt => SIGNER_SOURCE_PROMPT,
            Self::Filepath(_) => SIGNER_SOURCE_FILEPATH,
            Self::Usb(_) => SIGNER_SOURCE_USB,
            Self::Stdin => SIGNER_SOURCE_STDIN,
            Self::Pubkey(_) => SIGNER_SOURCE_PUBKEY,
        }
    }
}

impl std::fmt::Debug for SignerSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s: &str = self.as_ref();
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone)]
pub struct SignerSource {
    pub kind: SignerSourceKind,
    pub derivation_path: Option<DerivationPath>,
    pub legacy: bool,
}

impl SignerSource {
    fn new(kind: SignerSourceKind) -> Self {
        Self {
            kind,
            derivation_path: None,
            legacy: false,
        }
    }

    fn new_legacy(kind: SignerSourceKind) -> Self {
        Self {
            kind,
            derivation_path: None,
            legacy: true,
        }
    }

    pub fn try_get_keypair(
        matches: &ArgMatches,
        name: &str,
    ) -> Result<Option<Keypair>, Box<dyn error::Error>> {
        let source = matches.try_get_one::<Self>(name)?;
        if let Some(source) = source {
            keypair_from_source(matches, source, name, true).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn try_get_keypairs(
        matches: &ArgMatches,
        name: &str,
    ) -> Result<Option<Vec<Keypair>>, Box<dyn error::Error>> {
        let sources = matches.try_get_many::<Self>(name)?;
        if let Some(sources) = sources {
            let keypairs = sources
                .filter_map(|source| keypair_from_source(matches, source, name, true).ok())
                .collect();
            Ok(Some(keypairs))
        } else {
            Ok(None)
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn try_get_signer(
        matches: &ArgMatches,
        name: &str,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Option<(Box<dyn Signer>, Pubkey)>, Box<dyn error::Error>> {
        let source = matches.try_get_one::<Self>(name)?;
        if let Some(source) = source {
            let signer = signer_from_source(matches, source, name, wallet_manager)?;
            let signer_pubkey = signer.pubkey();
            Ok(Some((signer, signer_pubkey)))
        } else {
            Ok(None)
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn try_get_signers(
        matches: &ArgMatches,
        name: &str,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Option<Vec<(Box<dyn Signer>, Pubkey)>>, Box<dyn error::Error>> {
        let sources = matches.try_get_many::<Self>(name)?;
        if let Some(sources) = sources {
            let signers = sources
                .filter_map(|source| {
                    let signer = signer_from_source(matches, source, name, wallet_manager).ok()?;
                    let signer_pubkey = signer.pubkey();
                    Some((signer, signer_pubkey))
                })
                .collect();
            Ok(Some(signers))
        } else {
            Ok(None)
        }
    }

    pub fn try_get_pubkey(
        matches: &ArgMatches,
        name: &str,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Option<Pubkey>, Box<dyn error::Error>> {
        let source = matches.try_get_one::<Self>(name)?;
        if let Some(source) = source {
            pubkey_from_source(matches, source, name, wallet_manager).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn try_get_pubkeys(
        matches: &ArgMatches,
        name: &str,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Option<Vec<Pubkey>>, Box<dyn std::error::Error>> {
        let sources = matches.try_get_many::<Self>(name)?;
        if let Some(sources) = sources {
            let pubkeys = sources
                .filter_map(|source| pubkey_from_source(matches, source, name, wallet_manager).ok())
                .collect();
            Ok(Some(pubkeys))
        } else {
            Ok(None)
        }
    }

    pub fn try_resolve(
        matches: &ArgMatches,
        name: &str,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let source = matches.try_get_one::<Self>(name)?;
        if let Some(source) = source {
            resolve_signer_from_source(matches, source, name, wallet_manager)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn parse<S: AsRef<str>>(source: S) -> Result<SignerSource, SignerSourceError> {
        let source = source.as_ref();
        let source = {
            #[cfg(target_family = "windows")]
            {
                // trim matched single-quotes since cmd.exe won't
                let mut source = source;
                while let Some(trimmed) = source.strip_prefix('\'') {
                    source = if let Some(trimmed) = trimmed.strip_suffix('\'') {
                        trimmed
                    } else {
                        break;
                    }
                }
                source.replace('\\', "/")
            }
            #[cfg(not(target_family = "windows"))]
            {
                source.to_string()
            }
        };
        match uriparse::URIReference::try_from(source.as_str()) {
            Err(_) => Err(SignerSourceError::UnrecognizedSource),
            Ok(uri) => {
                if let Some(scheme) = uri.scheme() {
                    let scheme = scheme.as_str().to_ascii_lowercase();
                    match scheme.as_str() {
                        SIGNER_SOURCE_PROMPT => Ok(SignerSource {
                            kind: SignerSourceKind::Prompt,
                            derivation_path: DerivationPath::from_uri_any_query(&uri)?,
                            legacy: false,
                        }),
                        SIGNER_SOURCE_FILEPATH => Ok(SignerSource::new(
                            SignerSourceKind::Filepath(uri.path().to_string()),
                        )),
                        SIGNER_SOURCE_USB => Ok(SignerSource {
                            kind: SignerSourceKind::Usb(RemoteWalletLocator::new_from_uri(&uri)?),
                            derivation_path: DerivationPath::from_uri_key_query(&uri)?,
                            legacy: false,
                        }),
                        SIGNER_SOURCE_STDIN => Ok(SignerSource::new(SignerSourceKind::Stdin)),
                        _ => {
                            #[cfg(target_family = "windows")]
                            // On Windows, an absolute path's drive letter will be parsed as the URI
                            // scheme. Assume a filepath source in case of a single character shceme.
                            if scheme.len() == 1 {
                                return Ok(SignerSource::new(SignerSourceKind::Filepath(source)));
                            }
                            Err(SignerSourceError::UnrecognizedSource)
                        }
                    }
                } else {
                    match source.as_str() {
                        STDOUT_OUTFILE_TOKEN => Ok(SignerSource::new(SignerSourceKind::Stdin)),
                        ASK_KEYWORD => Ok(SignerSource::new_legacy(SignerSourceKind::Prompt)),
                        _ => match Pubkey::from_str(source.as_str()) {
                            Ok(pubkey) => Ok(SignerSource::new(SignerSourceKind::Pubkey(pubkey))),
                            Err(_) => std::fs::metadata(source.as_str())
                                .map(|_| SignerSource::new(SignerSourceKind::Filepath(source)))
                                .map_err(|err| err.into()),
                        },
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct SignerSourceParserBuilder {
    prompt: bool,
    file_path: bool,
    usb: bool,
    stdin: bool,
    pubkey: bool,
    allow_legacy: bool,
}

impl Default for SignerSourceParserBuilder {
    fn default() -> Self {
        Self {
            prompt: true,
            file_path: true,
            usb: true,
            stdin: true,
            pubkey: true,
            allow_legacy: true,
        }
    }
}

impl SignerSourceParserBuilder {
    pub fn new() -> Self {
        Self {
            prompt: false,
            file_path: false,
            usb: false,
            stdin: false,
            pubkey: false,
            allow_legacy: true,
        }
    }

    pub fn allow_prompt(mut self) -> Self {
        self.prompt = true;
        self
    }

    pub fn allow_file_path(mut self) -> Self {
        self.file_path = true;
        self
    }

    pub fn allow_usb(mut self) -> Self {
        self.usb = true;
        self
    }

    pub fn allow_stdin(mut self) -> Self {
        self.stdin = true;
        self
    }

    pub fn allow_pubkey(mut self) -> Self {
        self.pubkey = true;
        self
    }

    pub fn disallow_legacy(mut self) -> Self {
        self.allow_legacy = false;
        self
    }

    pub fn build(self) -> ValueParser {
        ValueParser::from(
            move |arg: &str| -> Result<SignerSource, SignerSourceError> {
                let signer_source = SignerSource::parse(arg)?;
                if !self.allow_legacy && signer_source.legacy {
                    return Err(SignerSourceError::UnsupportedSource);
                }
                match signer_source.kind {
                    SignerSourceKind::Prompt if self.prompt => Ok(signer_source),
                    SignerSourceKind::Filepath(_) if self.file_path => Ok(signer_source),
                    SignerSourceKind::Usb(_) if self.usb => Ok(signer_source),
                    SignerSourceKind::Stdin if self.stdin => Ok(signer_source),
                    SignerSourceKind::Pubkey(_) if self.pubkey => Ok(signer_source),
                    _ => Err(SignerSourceError::UnsupportedSource),
                }
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PubkeySignature {
    pub pubkey: Pubkey,
    pub signature: Signature,
}
impl FromStr for PubkeySignature {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut signer = s.split('=');
        let pubkey = signer
            .next()
            .ok_or_else(|| String::from("Malformed signer string"))?;
        let pubkey = Pubkey::from_str(pubkey).map_err(|err| format!("{err}"))?;

        let signature = signer
            .next()
            .ok_or_else(|| String::from("Malformed signer string"))?;
        let signature = Signature::from_str(signature).map_err(|err| format!("{err}"))?;

        Ok(Self { pubkey, signature })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        clap::{Arg, Command},
        solana_remote_wallet::locator::Manufacturer,
        tempfile::NamedTempFile,
    };

    #[test]
    fn test_parse_pubkey_signature() {
        let command = Command::new("test").arg(
            Arg::new("pubkeysig")
                .long("pubkeysig")
                .takes_value(true)
                .value_parser(clap::value_parser!(PubkeySignature)),
        );

        // success case
        let matches = command
            .clone()
            .try_get_matches_from(vec![
                "test",
                "--pubkeysig",
                "11111111111111111111111111111111=4TpFuec1u4BZfxgHg2VQXwvBHANZuNSJHmgrU34GViLAM5uYZ8t7uuhWMHN4k9r41B2p9mwnHjPGwTmTxyvCZw63"
                ]
            )
            .unwrap();

        let expected = PubkeySignature {
            pubkey: Pubkey::from_str("11111111111111111111111111111111").unwrap(),
            signature: Signature::from_str("4TpFuec1u4BZfxgHg2VQXwvBHANZuNSJHmgrU34GViLAM5uYZ8t7uuhWMHN4k9r41B2p9mwnHjPGwTmTxyvCZw63").unwrap(),
        };

        assert_eq!(
            *matches.get_one::<PubkeySignature>("pubkeysig").unwrap(),
            expected,
        );

        // validation fails
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--pubkeysig", "this_is_an_invalid_arg"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_keypair_source() {
        let command = Command::new("test").arg(
            Arg::new("keypair")
                .long("keypair")
                .takes_value(true)
                .value_parser(SignerSourceParserBuilder::new().allow_file_path().build()),
        );

        // success case
        let file0 = NamedTempFile::new().unwrap();
        let path = file0.path();
        let path_str = path.to_str().unwrap();

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", path_str])
            .unwrap();

        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();

        assert!(matches!(signer_source, SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            }
            if p == path_str));

        // faile cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "-"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "usb::/ledger"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_keypair_or_ask_keyword_source() {
        // allow `ASK` keyword
        let command = Command::new("test").arg(
            Arg::new("keypair")
                .long("keypair")
                .takes_value(true)
                .value_parser(
                    SignerSourceParserBuilder::new()
                        .allow_file_path()
                        .allow_prompt()
                        .build(),
                ),
        );

        // success cases
        let file0 = NamedTempFile::new().unwrap();
        let path = file0.path();
        let path_str = path.to_str().unwrap();

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", path_str])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();
        assert!(matches!(signer_source, SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            }
            if p == path_str));

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "ASK"])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();
        assert_matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: true,
            }
        );

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "prompt:"])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();
        assert_matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: false,
            }
        );

        // faile cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "-"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "usb::/ledger"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        // disallow `ASK` keyword
        let command = Command::new("test").arg(
            Arg::new("keypair")
                .long("keypair")
                .takes_value(true)
                .value_parser(
                    SignerSourceParserBuilder::new()
                        .allow_file_path()
                        .allow_prompt()
                        .disallow_legacy()
                        .build(),
                ),
        );

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "ASK"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_prompt_signer_source() {
        let command = Command::new("test").arg(
            Arg::new("keypair")
                .long("keypair")
                .takes_value(true)
                .value_parser(SignerSourceParserBuilder::new().allow_prompt().build()),
        );

        // success case
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "ASK"])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();
        assert_matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: true,
            }
        );

        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "prompt:"])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("keypair").unwrap();
        assert_matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: false,
            }
        );

        // faile cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "-"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--keypair", "usb::/ledger"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_pubkey_or_keypair_signer_source() {
        let command = Command::new("test").arg(
            Arg::new("signer")
                .long("signer")
                .takes_value(true)
                .value_parser(
                    SignerSourceParserBuilder::new()
                        .allow_pubkey()
                        .allow_file_path()
                        .build(),
                ),
        );

        // success cases
        let pubkey = Pubkey::new_unique();
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--signer", &pubkey.to_string()])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("signer").unwrap();
        assert!(matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Pubkey(p),
                derivation_path: None,
                legacy: false,
            }
            if *p == pubkey));

        let file0 = NamedTempFile::new().unwrap();
        let path = file0.path();
        let path_str = path.to_str().unwrap();
        let matches = command
            .clone()
            .try_get_matches_from(vec!["test", "--signer", path_str])
            .unwrap();
        let signer_source = matches.get_one::<SignerSource>("signer").unwrap();
        assert!(matches!(
            signer_source,
            SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            }
            if p == path_str));

        // faile cases
        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--signer", "-"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);

        let matches_error = command
            .clone()
            .try_get_matches_from(vec!["test", "--signer", "usb::/ledger"])
            .unwrap_err();
        assert_eq!(matches_error.kind, clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_parse_signer_source() {
        assert_matches!(
            SignerSource::parse(STDOUT_OUTFILE_TOKEN).unwrap(),
            SignerSource {
                kind: SignerSourceKind::Stdin,
                derivation_path: None,
                legacy: false,
            }
        );
        let stdin = "stdin:".to_string();
        assert_matches!(
            SignerSource::parse(stdin).unwrap(),
            SignerSource {
                kind: SignerSourceKind::Stdin,
                derivation_path: None,
                legacy: false,
            }
        );
        assert_matches!(
            SignerSource::parse(ASK_KEYWORD).unwrap(),
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: true,
            }
        );
        let pubkey = Pubkey::new_unique();
        assert!(
            matches!(SignerSource::parse(pubkey.to_string()).unwrap(), SignerSource {
                kind: SignerSourceKind::Pubkey(p),
                derivation_path: None,
                legacy: false,
            }
            if p == pubkey)
        );

        // Set up absolute and relative path strs
        let file0 = NamedTempFile::new().unwrap();
        let path = file0.path();
        assert!(path.is_absolute());
        let absolute_path_str = path.to_str().unwrap();

        let file1 = NamedTempFile::new_in(std::env::current_dir().unwrap()).unwrap();
        let path = file1.path().file_name().unwrap().to_str().unwrap();
        let path = std::path::Path::new(path);
        assert!(path.is_relative());
        let relative_path_str = path.to_str().unwrap();

        assert!(
            matches!(SignerSource::parse(absolute_path_str).unwrap(), SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            } if p == absolute_path_str)
        );
        assert!(
            matches!(SignerSource::parse(relative_path_str).unwrap(), SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            } if p == relative_path_str)
        );

        let usb = "usb://ledger".to_string();
        let expected_locator = RemoteWalletLocator {
            manufacturer: Manufacturer::Ledger,
            pubkey: None,
        };
        assert_matches!(SignerSource::parse(usb).unwrap(), SignerSource {
                kind: SignerSourceKind::Usb(u),
                derivation_path: None,
                legacy: false,
            } if u == expected_locator);
        let usb = "usb://ledger?key=0/0".to_string();
        let expected_locator = RemoteWalletLocator {
            manufacturer: Manufacturer::Ledger,
            pubkey: None,
        };
        let expected_derivation_path = Some(DerivationPath::new_bip44(Some(0), Some(0)));
        assert_matches!(SignerSource::parse(usb).unwrap(), SignerSource {
                kind: SignerSourceKind::Usb(u),
                derivation_path: d,
                legacy: false,
            } if u == expected_locator && d == expected_derivation_path);
        // Catchall into SignerSource::Filepath fails
        let junk = "sometextthatisnotapubkeyorfile".to_string();
        assert!(Pubkey::from_str(&junk).is_err());
        assert_matches!(
            SignerSource::parse(&junk),
            Err(SignerSourceError::IoError(_))
        );

        let prompt = "prompt:".to_string();
        assert_matches!(
            SignerSource::parse(prompt).unwrap(),
            SignerSource {
                kind: SignerSourceKind::Prompt,
                derivation_path: None,
                legacy: false,
            }
        );
        assert!(
            matches!(SignerSource::parse(format!("file:{absolute_path_str}")).unwrap(), SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            } if p == absolute_path_str)
        );
        assert!(
            matches!(SignerSource::parse(format!("file:{relative_path_str}")).unwrap(), SignerSource {
                kind: SignerSourceKind::Filepath(p),
                derivation_path: None,
                legacy: false,
            } if p == relative_path_str)
        );
    }
}
