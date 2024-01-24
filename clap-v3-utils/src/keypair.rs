//! Loading signers and keypairs from the command line.
//!
//! This module contains utilities for loading [Signer]s and [Keypair]s from
//! standard signing sources, from the command line, as in the Solana CLI.
//!
//! The key function here is [`signer_from_path`], which loads a `Signer` from
//! one of several possible sources by interpreting a "path" command line
//! argument. Its documentation includes a description of all possible signing
//! sources supported by the Solana CLI. Many other functions here are
//! variations on, or delegate to, `signer_from_path`.

use {
    crate::{
        input_parsers::signer::{try_pubkeys_sigs_of, SignerSource, SignerSourceKind},
        offline::{SIGNER_ARG, SIGN_ONLY_ARG},
        ArgConstant,
    },
    bip39::{Language, Mnemonic, Seed},
    clap::ArgMatches,
    rpassword::prompt_password,
    solana_remote_wallet::{
        remote_keypair::generate_remote_keypair,
        remote_wallet::{maybe_wallet_manager, RemoteWalletError, RemoteWalletManager},
    },
    solana_sdk::{
        derivation_path::DerivationPath,
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::{
            generate_seed_from_seed_phrase_and_passphrase, read_keypair, read_keypair_file,
            EncodableKey, EncodableKeypair, Keypair, NullSigner, Presigner, SeedDerivable,
            Signature, Signer,
        },
    },
    solana_zk_token_sdk::encryption::{auth_encryption::AeKey, elgamal::ElGamalKeypair},
    std::{
        cell::RefCell,
        error,
        io::{stdin, stdout, Write},
        ops::Deref,
        process::exit,
        rc::Rc,
    },
};

pub struct SignOnly {
    pub blockhash: Hash,
    pub message: Option<String>,
    pub present_signers: Vec<(Pubkey, Signature)>,
    pub absent_signers: Vec<Pubkey>,
    pub bad_signers: Vec<Pubkey>,
}

impl SignOnly {
    pub fn has_all_signers(&self) -> bool {
        self.absent_signers.is_empty() && self.bad_signers.is_empty()
    }

    pub fn presigner_of(&self, pubkey: &Pubkey) -> Option<Presigner> {
        presigner_from_pubkey_sigs(pubkey, &self.present_signers)
    }
}
pub type CliSigners = Vec<Box<dyn Signer>>;
pub type SignerIndex = usize;
pub struct CliSignerInfo {
    pub signers: CliSigners,
}

impl CliSignerInfo {
    pub fn index_of(&self, pubkey: Option<Pubkey>) -> Option<usize> {
        if let Some(pubkey) = pubkey {
            self.signers
                .iter()
                .position(|signer| signer.pubkey() == pubkey)
        } else {
            Some(0)
        }
    }
    pub fn index_of_or_none(&self, pubkey: Option<Pubkey>) -> Option<usize> {
        if let Some(pubkey) = pubkey {
            self.signers
                .iter()
                .position(|signer| signer.pubkey() == pubkey)
        } else {
            None
        }
    }
    pub fn signers_for_message(&self, message: &Message) -> Vec<&dyn Signer> {
        self.signers
            .iter()
            .filter_map(|k| {
                if message.signer_keys().contains(&&k.pubkey()) {
                    Some(k.as_ref())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// A command line argument that loads a default signer in absence of other signers.
///
/// This type manages a default signing source which may be overridden by other
/// signing sources via its [`generate_unique_signers`] method.
///
/// [`generate_unique_signers`]: DefaultSigner::generate_unique_signers
///
/// `path` is a signing source as documented by [`signer_from_path`], and
/// `arg_name` is the name of its [clap] command line argument, which is passed
/// to `signer_from_path` as its `keypair_name` argument.
#[derive(Debug, Default)]
pub struct DefaultSigner {
    /// The name of the signers command line argument.
    pub arg_name: String,
    /// The signing source.
    pub path: String,
    is_path_checked: RefCell<bool>,
}

impl DefaultSigner {
    /// Create a new `DefaultSigner`.
    ///
    /// `path` is a signing source as documented by [`signer_from_path`], and
    /// `arg_name` is the name of its [clap] command line argument, which is
    /// passed to `signer_from_path` as its `keypair_name` argument.
    ///
    /// [clap]: https://docs.rs/clap
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use clap::{Arg, Command};
    /// use solana_clap_v3_utils::keypair::DefaultSigner;
    /// use solana_clap_v3_utils::offline::OfflineArgs;
    ///
    /// let clap_app = Command::new("my-program")
    ///     // The argument we'll parse as a signer "path"
    ///     .arg(Arg::new("keypair")
    ///         .required(true)
    ///         .help("The default signer"))
    ///     .offline_args();
    ///
    /// let clap_matches = clap_app.get_matches();
    /// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
    ///
    /// let default_signer = DefaultSigner::new("keypair", &keypair_str);
    /// # assert!(default_signer.arg_name.len() > 0);
    /// assert_eq!(default_signer.path, keypair_str);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new<AN: AsRef<str>, P: AsRef<str>>(arg_name: AN, path: P) -> Self {
        let arg_name = arg_name.as_ref().to_string();
        let path = path.as_ref().to_string();
        Self {
            arg_name,
            path,
            ..Self::default()
        }
    }

    fn path(&self) -> Result<&str, Box<dyn std::error::Error>> {
        if !self.is_path_checked.borrow().deref() {
            SignerSource::parse(&self.path)
                .and_then(|s| {
                    if let SignerSourceKind::Filepath(path) = &s.kind {
                        std::fs::metadata(path).map(|_| ()).map_err(|e| e.into())
                    } else {
                        Ok(())
                    }
                })
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!(
                        "No default signer found, run \"solana-keygen new -o {}\" to create a new one",
                        self.path
                    ),
                    )
                })?;
            *self.is_path_checked.borrow_mut() = true;
        }
        Ok(&self.path)
    }

    /// Generate a unique set of signers, possibly excluding this default signer.
    ///
    /// This function allows a command line application to have a default
    /// signer, perhaps representing a default wallet, but to override that
    /// signer and instead sign with one or more other signers.
    ///
    /// `bulk_signers` is a vector of signers, all of which are optional. If any
    /// of those signers is `None`, then the default signer will be loaded; if
    /// all of those signers are `Some`, then the default signer will not be
    /// loaded.
    ///
    /// The returned value includes all of the `bulk_signers` that were not
    /// `None`, and maybe the default signer, if it was loaded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use clap::{Arg, Command};
    /// use solana_clap_v3_utils::keypair::{DefaultSigner, signer_from_path};
    /// use solana_clap_v3_utils::offline::OfflineArgs;
    /// use solana_sdk::signer::Signer;
    ///
    /// let clap_app = Command::new("my-program")
    ///     // The argument we'll parse as a signer "path"
    ///     .arg(Arg::new("keypair")
    ///         .required(true)
    ///         .help("The default signer"))
    ///     .arg(Arg::new("payer")
    ///         .long("payer")
    ///         .help("The account paying for the transaction"))
    ///     .offline_args();
    ///
    /// let mut wallet_manager = None;
    ///
    /// let clap_matches = clap_app.get_matches();
    /// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
    /// let maybe_payer = clap_matches.value_of("payer");
    ///
    /// let default_signer = DefaultSigner::new("keypair", &keypair_str);
    /// let maybe_payer_signer = maybe_payer.map(|payer| {
    ///     signer_from_path(&clap_matches, payer, "payer", &mut wallet_manager)
    /// }).transpose()?;
    /// let bulk_signers = vec![maybe_payer_signer];
    ///
    /// let unique_signers = default_signer.generate_unique_signers(
    ///     bulk_signers,
    ///     &clap_matches,
    ///     &mut wallet_manager,
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn generate_unique_signers(
        &self,
        bulk_signers: Vec<Option<Box<dyn Signer>>>,
        matches: &ArgMatches,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<CliSignerInfo, Box<dyn error::Error>> {
        let mut unique_signers = vec![];

        // Determine if the default signer is needed
        if bulk_signers.iter().any(|signer| signer.is_none()) {
            let default_signer = self.signer_from_path(matches, wallet_manager)?;
            unique_signers.push(default_signer);
        }

        for signer in bulk_signers.into_iter().flatten() {
            if !unique_signers.iter().any(|s| s == &signer) {
                unique_signers.push(signer);
            }
        }
        Ok(CliSignerInfo {
            signers: unique_signers,
        })
    }

    /// Loads the default [Signer] from one of several possible sources.
    ///
    /// The `path` is not strictly a file system path, but is interpreted as
    /// various types of _signing source_, depending on its format, one of which
    /// is a path to a keypair file. Some sources may require user interaction
    /// in the course of calling this function.
    ///
    /// This simply delegates to the [`signer_from_path`] free function, passing
    /// it the `DefaultSigner`s `path` and `arg_name` fields as the `path` and
    /// `keypair_name` arguments.
    ///
    /// See the [`signer_from_path`] free function for full documentation of how
    /// this function interprets its arguments.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use clap::{Arg, Command};
    /// use solana_clap_v3_utils::keypair::DefaultSigner;
    /// use solana_clap_v3_utils::offline::OfflineArgs;
    ///
    /// let clap_app = Command::new("my-program")
    ///     // The argument we'll parse as a signer "path"
    ///     .arg(Arg::new("keypair")
    ///         .required(true)
    ///         .help("The default signer"))
    ///     .offline_args();
    ///
    /// let clap_matches = clap_app.get_matches();
    /// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
    /// let default_signer = DefaultSigner::new("keypair", &keypair_str);
    /// let mut wallet_manager = None;
    ///
    /// let signer = default_signer.signer_from_path(
    ///     &clap_matches,
    ///     &mut wallet_manager,
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn signer_from_path(
        &self,
        matches: &ArgMatches,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    ) -> Result<Box<dyn Signer>, Box<dyn std::error::Error>> {
        signer_from_path(matches, self.path()?, &self.arg_name, wallet_manager)
    }

    /// Loads the default [Signer] from one of several possible sources.
    ///
    /// The `path` is not strictly a file system path, but is interpreted as
    /// various types of _signing source_, depending on its format, one of which
    /// is a path to a keypair file. Some sources may require user interaction
    /// in the course of calling this function.
    ///
    /// This simply delegates to the [`signer_from_path_with_config`] free
    /// function, passing it the `DefaultSigner`s `path` and `arg_name` fields
    /// as the `path` and `keypair_name` arguments.
    ///
    /// See the [`signer_from_path`] free function for full documentation of how
    /// this function interprets its arguments.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use clap::{Arg, Command};
    /// use solana_clap_v3_utils::keypair::{SignerFromPathConfig, DefaultSigner};
    /// use solana_clap_v3_utils::offline::OfflineArgs;
    ///
    /// let clap_app = Command::new("my-program")
    ///     // The argument we'll parse as a signer "path"
    ///     .arg(Arg::new("keypair")
    ///         .required(true)
    ///         .help("The default signer"))
    ///     .offline_args();
    ///
    /// let clap_matches = clap_app.get_matches();
    /// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
    /// let default_signer = DefaultSigner::new("keypair", &keypair_str);
    /// let mut wallet_manager = None;
    ///
    /// // Allow pubkey signers without accompanying signatures
    /// let config = SignerFromPathConfig {
    ///     allow_null_signer: true,
    /// };
    ///
    /// let signer = default_signer.signer_from_path_with_config(
    ///     &clap_matches,
    ///     &mut wallet_manager,
    ///     &config,
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn signer_from_path_with_config(
        &self,
        matches: &ArgMatches,
        wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
        config: &SignerFromPathConfig,
    ) -> Result<Box<dyn Signer>, Box<dyn std::error::Error>> {
        signer_from_path_with_config(
            matches,
            self.path()?,
            &self.arg_name,
            wallet_manager,
            config,
        )
    }
}

pub fn presigner_from_pubkey_sigs(
    pubkey: &Pubkey,
    signers: &[(Pubkey, Signature)],
) -> Option<Presigner> {
    signers.iter().find_map(|(signer, sig)| {
        if *signer == *pubkey {
            Some(Presigner::new(signer, sig))
        } else {
            None
        }
    })
}

#[derive(Debug, Default)]
pub struct SignerFromPathConfig {
    pub allow_null_signer: bool,
}

/// Loads a [Signer] from one of several possible sources.
///
/// The `path` is not strictly a file system path, but is interpreted as various
/// types of _signing source_, depending on its format, one of which is a path
/// to a keypair file. Some sources may require user interaction in the course
/// of calling this function.
///
/// The result of this function is a boxed object of the [Signer] trait. To load
/// a concrete [Keypair], use the [keypair_from_path] function, though note that
/// it does not support all signer sources.
///
/// The `matches` argument is the same set of parsed [clap] matches from which
/// `path` was parsed. It is used to parse various additional command line
/// arguments, depending on which signing source is requested, as described
/// below in "Signing sources".
///
/// [clap]: https//docs.rs/clap
///
/// The `keypair_name` argument is the "name" of the signer, and is typically
/// the name of the clap argument from which the `path` argument was parsed,
/// like "keypair", "from", or "fee-payer". It is used solely for interactively
/// prompting the user, either when entering seed phrases or selecting from
/// multiple hardware wallets.
///
/// The `wallet_manager` is used for establishing connections to a hardware
/// device such as Ledger. If `wallet_manager` is a reference to `None`, and a
/// hardware signer is requested, then this function will attempt to create a
/// wallet manager, assigning it to the mutable `wallet_manager` reference. This
/// argument is typically a reference to `None`.
///
/// # Signing sources
///
/// The `path` argument can simply be a path to a keypair file, but it may also
/// be interpreted in several other ways, in the following order.
///
/// Firstly, the `path` argument may be interpreted as a [URI], with the URI
/// scheme indicating where to load the signer from. If it parses as a URI, then
/// the following schemes are supported:
///
/// - `file:` &mdash; Read the keypair from a JSON keypair file. The path portion
///    of the URI is the file path.
///
/// - `stdin:` &mdash; Read the keypair from stdin, in the JSON format used by
///   the keypair file.
///
///   Non-scheme parts of the URI are ignored.
///
/// - `prompt:` &mdash; The user will be prompted at the command line
///   for their seed phrase and passphrase.
///
///   In this URI the [query string][qs] may contain zero or one of the
///   following key/value pairs that determine the [BIP44 derivation path][dp]
///   of the private key from the seed:
///
///   - `key` &mdash; In this case the value is either one or two numerical
///     indexes separated by a slash, which represent the "account", and
///     "change" components of the BIP44 derivation path. Example: `key=0/0`.
///
///   - `full-path` &mdash; In this case the value is a full derivation path,
///     and the user is responsible for ensuring it is correct. Example:
///     `full-path=m/44/501/0/0/0`.
///
///   If neither is provided, then the default derivation path is used.
///
///   Note that when specifying derivation paths, this routine will convert all
///   indexes into ["hardened"] indexes, even if written as "normal" indexes.
///
///   Other components of the URI besides the scheme and query string are ignored.
///
///   If the "skip_seed_phrase_validation" argument, as defined in
///   [SKIP_SEED_PHRASE_VALIDATION_ARG] is found in `matches`, then the keypair
///   seed will be generated directly from the seed phrase, without parsing or
///   validating it as a BIP39 seed phrase. This allows the use of non-BIP39 seed
///   phrases.
///
/// - `usb:` &mdash; Use a USB hardware device as the signer. In this case, the
///   URI host indicates the device type, and is required. The only currently valid host
///   value is "ledger".
///
///   Optionally, the first segment of the URI path indicates the base-58
///   encoded pubkey of the wallet, and the "account" and "change" indices of
///   the derivation path can be specified with the `key=` query parameter, as
///   with the `prompt:` URI.
///
///   Examples:
///
///   - `usb://ledger`
///   - `usb://ledger?key=0/0`
///   - `usb://ledger/9rPVSygg3brqghvdZ6wsL2i5YNQTGhXGdJzF65YxaCQd`
///   - `usb://ledger/9rPVSygg3brqghvdZ6wsL2i5YNQTGhXGdJzF65YxaCQd?key=0/0`
///
/// Next the `path` argument may be one of the following strings:
///
/// - `-` &mdash; Read the keypair from stdin. This is the same as the `stdin:`
///   URI scheme.
///
/// - `ASK` &mdash; The user will be prompted at the command line for their seed
///   phrase and passphrase. _This uses a legacy key derivation method and should
///   usually be avoided in favor of `prompt:`._
///
/// Next, if the `path` argument parses as a base-58 public key, then the signer
/// is created without a private key, but with presigned signatures, each parsed
/// from the additional command line arguments, provided by the `matches`
/// argument.
///
/// In this case, the remaining command line arguments are searched for clap
/// arguments named "signer", as defined by [SIGNER_ARG], and each is parsed as
/// a key-value pair of the form "pubkey=signature", where `pubkey` is the same
/// base-58 public key, and `signature` is a serialized signature produced by
/// the corresponding keypair. One of the "signer" signatures must be for the
/// pubkey specified in `path` or this function will return an error; unless the
/// "sign_only" clap argument, as defined by [SIGN_ONLY_ARG], is present in
/// `matches`, in which case the signer will be created with no associated
/// signatures.
///
/// Finally, if `path`, interpreted as a file path, represents a file on disk,
/// then the signer is created by reading that file as a JSON-serialized
/// keypair. This is the same as the `file:` URI scheme.
///
/// [qs]: https://en.wikipedia.org/wiki/Query_string
/// [dp]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
/// [URI]: https://en.wikipedia.org/wiki/Uniform_Resource_Identifier
/// ["hardened"]: https://wiki.trezor.io/Hardened_and_non-hardened_derivation
///
/// # Examples
///
/// This shows a reasonable way to set up clap to parse all possible signer
/// sources. Note the use of the [`OfflineArgs::offline_args`] method to add
/// correct clap definitions of the `--signer` and `--sign-only` arguments, as
/// required by the base-58 pubkey offline signing method.
///
/// [`OfflineArgs::offline_args`]: crate::offline::OfflineArgs::offline_args
///
/// ```no_run
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::signer_from_path;
/// use solana_clap_v3_utils::offline::OfflineArgs;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("keypair")
///         .required(true)
///         .help("The default signer"))
///     .offline_args();
///
/// let clap_matches = clap_app.get_matches();
/// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
/// let mut wallet_manager = None;
/// let signer = signer_from_path(
///     &clap_matches,
///     &keypair_str,
///     "keypair",
///     &mut wallet_manager,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn signer_from_path(
    matches: &ArgMatches,
    path: &str,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let config = SignerFromPathConfig::default();
    signer_from_path_with_config(matches, path, keypair_name, wallet_manager, &config)
}

pub fn signer_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let config = SignerFromPathConfig::default();
    signer_from_source_with_config(matches, source, keypair_name, wallet_manager, &config)
}

/// Loads a [Signer] from one of several possible sources.
///
/// The `path` is not strictly a file system path, but is interpreted as various
/// types of _signing source_, depending on its format, one of which is a path
/// to a keypair file. Some sources may require user interaction in the course
/// of calling this function.
///
/// This is the same as [`signer_from_path`] except that it additionaolly
/// accepts a [`SignerFromPathConfig`] argument.
///
/// If the `allow_null_signer` field of `config` is `true`, then pubkey signers
/// are allowed to have zero associated signatures via additional "signer"
/// command line arguments. It the same effect as if the "sign_only" clap
/// argument is present.
///
/// See [`signer_from_path`] for full documentation of how this function
/// interprets its arguments.
///
/// # Examples
///
/// This shows a reasonable way to set up clap to parse all possible signer
/// sources. Note the use of the [`OfflineArgs::offline_args`] method to add
/// correct clap definitions of the `--signer` and `--sign-only` arguments, as
/// required by the base-58 pubkey offline signing method.
///
/// [`OfflineArgs::offline_args`]: crate::offline::OfflineArgs::offline_args
///
/// ```no_run
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::{signer_from_path_with_config, SignerFromPathConfig};
/// use solana_clap_v3_utils::offline::OfflineArgs;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("keypair")
///         .required(true)
///         .help("The default signer"))
///     .offline_args();
///
/// let clap_matches = clap_app.get_matches();
/// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
/// let mut wallet_manager = None;
///
/// // Allow pubkey signers without accompanying signatures
/// let config = SignerFromPathConfig {
///     allow_null_signer: true,
/// };
///
/// let signer = signer_from_path_with_config(
///     &clap_matches,
///     &keypair_str,
///     "keypair",
///     &mut wallet_manager,
///     &config,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn signer_from_path_with_config(
    matches: &ArgMatches,
    path: &str,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    config: &SignerFromPathConfig,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let source = SignerSource::parse(path)?;
    signer_from_source_with_config(matches, &source, keypair_name, wallet_manager, config)
}

pub fn signer_from_source_with_config(
    matches: &ArgMatches,
    source: &SignerSource,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
    config: &SignerFromPathConfig,
) -> Result<Box<dyn Signer>, Box<dyn error::Error>> {
    let SignerSource {
        kind,
        derivation_path,
        legacy,
    } = source;
    match kind {
        SignerSourceKind::Prompt => {
            let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
            Ok(Box::new(keypair_from_seed_phrase(
                keypair_name,
                skip_validation,
                false,
                derivation_path.clone(),
                *legacy,
            )?))
        }
        SignerSourceKind::Filepath(path) => match read_keypair_file(path) {
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("could not read keypair file \"{path}\". Run \"solana-keygen new\" to create a keypair file: {e}"),
            )
            .into()),
            Ok(file) => Ok(Box::new(file)),
        },
        SignerSourceKind::Stdin => {
            let mut stdin = std::io::stdin();
            Ok(Box::new(read_keypair(&mut stdin)?))
        }
        SignerSourceKind::Usb(locator) => {
            if wallet_manager.is_none() {
                *wallet_manager = maybe_wallet_manager()?;
            }
            if let Some(wallet_manager) = wallet_manager {
                let confirm_key = matches.try_contains_id("confirm_key").unwrap_or(false);
                Ok(Box::new(generate_remote_keypair(
                    locator.clone(),
                    derivation_path.clone().unwrap_or_default(),
                    wallet_manager,
                    confirm_key,
                    keypair_name,
                )?))
            } else {
                Err(RemoteWalletError::NoDeviceFound.into())
            }
        }
        SignerSourceKind::Pubkey(pubkey) => {
            let presigner = try_pubkeys_sigs_of(matches, SIGNER_ARG.name)?
                .as_ref()
                .and_then(|presigners| presigner_from_pubkey_sigs(pubkey, presigners));
            if let Some(presigner) = presigner {
                Ok(Box::new(presigner))
            } else if config.allow_null_signer || matches.try_contains_id(SIGN_ONLY_ARG.name)? {
                Ok(Box::new(NullSigner::new(pubkey)))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("missing signature for supplied pubkey: {pubkey}"),
                )
                .into())
            }
        }
    }
}

/// Loads the pubkey of a [Signer] from one of several possible sources.
///
/// The `path` is not strictly a file system path, but is interpreted as various
/// types of _signing source_, depending on its format, one of which is a path
/// to a keypair file. Some sources may require user interaction in the course
/// of calling this function.
///
/// The only difference between this function and [`signer_from_path`] is in the
/// case of a "pubkey" path: this function does not require that accompanying
/// command line arguments contain an offline signature.
///
/// See [`signer_from_path`] for full documentation of how this function
/// interprets its arguments.
///
/// # Examples
///
/// ```no_run
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::pubkey_from_path;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("keypair")
///         .required(true)
///         .help("The default signer"));
///
/// let clap_matches = clap_app.get_matches();
/// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
/// let mut wallet_manager = None;
/// let pubkey = pubkey_from_path(
///     &clap_matches,
///     &keypair_str,
///     "keypair",
///     &mut wallet_manager,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn pubkey_from_path(
    matches: &ArgMatches,
    path: &str,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Pubkey, Box<dyn error::Error>> {
    let source = SignerSource::parse(path)?;
    pubkey_from_source(matches, &source, keypair_name, wallet_manager)
}

pub fn pubkey_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Pubkey, Box<dyn error::Error>> {
    match source.kind {
        SignerSourceKind::Pubkey(pubkey) => Ok(pubkey),
        _ => Ok(signer_from_source(matches, source, keypair_name, wallet_manager)?.pubkey()),
    }
}

pub fn resolve_signer_from_path(
    matches: &ArgMatches,
    path: &str,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Option<String>, Box<dyn error::Error>> {
    let source = SignerSource::parse(path)?;
    resolve_signer_from_source(matches, &source, keypair_name, wallet_manager)
}

pub fn resolve_signer_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<Option<String>, Box<dyn error::Error>> {
    let SignerSource {
        kind,
        derivation_path,
        legacy,
    } = source;
    match kind {
        SignerSourceKind::Prompt => {
            let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
            // This method validates the seed phrase, but returns `None` because there is no path
            // on disk or to a device
            keypair_from_seed_phrase(
                keypair_name,
                skip_validation,
                false,
                derivation_path.clone(),
                *legacy,
            )
            .map(|_| None)
        }
        SignerSourceKind::Filepath(path) => match read_keypair_file(path) {
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "could not read keypair file \"{path}\". \
                    Run \"solana-keygen new\" to create a keypair file: {e}"
                ),
            )
            .into()),
            Ok(_) => Ok(Some(path.to_string())),
        },
        SignerSourceKind::Stdin => {
            let mut stdin = std::io::stdin();
            // This method validates the keypair from stdin, but returns `None` because there is no
            // path on disk or to a device
            read_keypair(&mut stdin).map(|_| None)
        }
        SignerSourceKind::Usb(locator) => {
            if wallet_manager.is_none() {
                *wallet_manager = maybe_wallet_manager()?;
            }
            if let Some(wallet_manager) = wallet_manager {
                let confirm_key = matches.try_contains_id("confirm_key").unwrap_or(false);
                let path = generate_remote_keypair(
                    locator.clone(),
                    derivation_path.clone().unwrap_or_default(),
                    wallet_manager,
                    confirm_key,
                    keypair_name,
                )
                .map(|keypair| keypair.path)?;
                Ok(Some(path))
            } else {
                Err(RemoteWalletError::NoDeviceFound.into())
            }
        }
        SignerSourceKind::Pubkey(pubkey) => Ok(Some(pubkey.to_string())),
    }
}

// Keyword used to indicate that the user should be prompted for a keypair seed phrase
pub const ASK_KEYWORD: &str = "ASK";

pub const SKIP_SEED_PHRASE_VALIDATION_ARG: ArgConstant<'static> = ArgConstant {
    long: "skip-seed-phrase-validation",
    name: "skip_seed_phrase_validation",
    help: "Skip validation of seed phrases. Use this if your phrase does not use the BIP39 official English word list",
};

/// Prompts user for a passphrase and then asks for confirmirmation to check for mistakes
pub fn prompt_passphrase(prompt: &str) -> Result<String, Box<dyn error::Error>> {
    let passphrase = prompt_password(prompt)?;
    if !passphrase.is_empty() {
        let confirmed = rpassword::prompt_password("Enter same passphrase again: ")?;
        if confirmed != passphrase {
            return Err("Passphrases did not match".into());
        }
    }
    Ok(passphrase)
}

/// Loads a [Keypair] from one of several possible sources.
///
/// The `path` is not strictly a file system path, but is interpreted as various
/// types of _signing source_, depending on its format, one of which is a path
/// to a keypair file. Some sources may require user interaction in the course
/// of calling this function.
///
/// This is the same as [`signer_from_path`] except that it only supports
/// signing sources that can result in a [Keypair]: prompt for seed phrase,
/// keypair file, and stdin.
///
/// If `confirm_pubkey` is `true` then after deriving the pubkey, the user will
/// be prompted to confirm that the pubkey is as expected.
///
/// See [`signer_from_path`] for full documentation of how this function
/// interprets its arguments.
///
/// # Examples
///
/// ```no_run
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::keypair_from_path;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("keypair")
///         .required(true)
///         .help("The default signer"));
///
/// let clap_matches = clap_app.get_matches();
/// let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");
///
/// let signer = keypair_from_path(
///     &clap_matches,
///     &keypair_str,
///     "keypair",
///     false,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn keypair_from_path(
    matches: &ArgMatches,
    path: &str,
    keypair_name: &str,
    confirm_pubkey: bool,
) -> Result<Keypair, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    let keypair = encodable_key_from_path(path, keypair_name, skip_validation)?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&keypair, "pubkey");
    }
    Ok(keypair)
}

pub fn keypair_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    keypair_name: &str,
    confirm_pubkey: bool,
) -> Result<Keypair, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    let keypair = encodable_key_from_source(source, keypair_name, skip_validation)?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&keypair, "pubkey");
    }
    Ok(keypair)
}

/// Loads an [ElGamalKeypair] from one of several possible sources.
///
/// If `confirm_pubkey` is `true` then after deriving the keypair, the user will
/// be prompted to confirm that the ElGamal pubkey is as expected.
///
/// The way this function interprets its arguments is analogous to that of
/// [`signer_from_path`].
///
/// The bip32 hierarchical derivation of an ElGamal keypair is not currently
/// supported.
///
/// # Examples
///
/// ```no_run`
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::elgamal_keypair_from_path;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("elgamal-keypair")
///         .required(true)
///         .help("The default signer"));
///
/// let clap_matches = clap_app.get_matches();
/// let elgamal_keypair_str: String = clap_matches.value_of_t_or_exit("elgamal-keypair");
///
/// let elgamal_keypair = elgamal_keypair_from_path(
///     &clap_matches,
///     &elgamal_keypair_str,
///     "elgamal-keypair",
///     false,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn elgamal_keypair_from_path(
    matches: &ArgMatches,
    path: &str,
    elgamal_keypair_name: &str,
    confirm_pubkey: bool,
) -> Result<ElGamalKeypair, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    let elgamal_keypair = encodable_key_from_path(path, elgamal_keypair_name, skip_validation)?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&elgamal_keypair, "ElGamal pubkey");
    }
    Ok(elgamal_keypair)
}

pub fn elgamal_keypair_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    elgamal_keypair_name: &str,
    confirm_pubkey: bool,
) -> Result<ElGamalKeypair, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    let elgamal_keypair = encodable_key_from_source(source, elgamal_keypair_name, skip_validation)?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&elgamal_keypair, "ElGamal pubkey");
    }
    Ok(elgamal_keypair)
}

fn confirm_encodable_keypair_pubkey<K: EncodableKeypair>(keypair: &K, pubkey_label: &str) {
    let pubkey = keypair.encodable_pubkey().to_string();
    println!("Recovered {pubkey_label} `{pubkey:?}`. Continue? (y/n): ");
    let _ignored = stdout().flush();
    let mut input = String::new();
    stdin().read_line(&mut input).expect("Unexpected input");
    if input.to_lowercase().trim() != "y" {
        println!("Exiting");
        exit(1);
    }
}

/// Loads an [AeKey] from one of several possible sources.
///
/// The way this function interprets its arguments is analogous to that of
/// [`signer_from_path`].
///
/// The bip32 hierarchical derivation of an authenticated encryption key is not
/// currently supported.
///
/// # Examples
///
/// ```no_run`
/// use clap::{Arg, Command};
/// use solana_clap_v3_utils::keypair::ae_key_from_path;
///
/// let clap_app = Command::new("my-program")
///     // The argument we'll parse as a signer "path"
///     .arg(Arg::new("ae-key")
///         .required(true)
///         .help("The default signer"));
///
/// let clap_matches = clap_app.get_matches();
/// let ae_key_str: String = clap_matches.value_of_t_or_exit("ae-key");
///
/// let ae_key = ae_key_from_path(
///     &clap_matches,
///     &ae_key_str,
///     "ae-key",
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn ae_key_from_path(
    matches: &ArgMatches,
    path: &str,
    key_name: &str,
) -> Result<AeKey, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    encodable_key_from_path(path, key_name, skip_validation)
}

pub fn ae_key_from_source(
    matches: &ArgMatches,
    source: &SignerSource,
    key_name: &str,
) -> Result<AeKey, Box<dyn error::Error>> {
    let skip_validation = matches.try_contains_id(SKIP_SEED_PHRASE_VALIDATION_ARG.name)?;
    encodable_key_from_source(source, key_name, skip_validation)
}

fn encodable_key_from_path<K: EncodableKey + SeedDerivable>(
    path: &str,
    keypair_name: &str,
    skip_validation: bool,
) -> Result<K, Box<dyn error::Error>> {
    let source = SignerSource::parse(path)?;
    encodable_key_from_source(&source, keypair_name, skip_validation)
}

fn encodable_key_from_source<K: EncodableKey + SeedDerivable>(
    source: &SignerSource,
    keypair_name: &str,
    skip_validation: bool,
) -> Result<K, Box<dyn error::Error>> {
    let SignerSource {
        kind,
        derivation_path,
        legacy,
    } = source;
    match kind {
        SignerSourceKind::Prompt => Ok(encodable_key_from_seed_phrase(
            keypair_name,
            skip_validation,
            derivation_path.clone(),
            *legacy,
        )?),
        SignerSourceKind::Filepath(path) => match K::read_from_file(path) {
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "could not read keypair file \"{path}\". \
                    Run \"solana-keygen new\" to create a keypair file: {e}"
                ),
            )
            .into()),
            Ok(file) => Ok(file),
        },
        SignerSourceKind::Stdin => {
            let mut stdin = std::io::stdin();
            Ok(K::read(&mut stdin)?)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("signer of type `{kind:?}` does not support Keypair output"),
        )
        .into()),
    }
}

/// Reads user input from stdin to retrieve a seed phrase and passphrase for keypair derivation.
///
/// Optionally skips validation of seed phrase. Optionally confirms recovered
/// public key.
pub fn keypair_from_seed_phrase(
    keypair_name: &str,
    skip_validation: bool,
    confirm_pubkey: bool,
    derivation_path: Option<DerivationPath>,
    legacy: bool,
) -> Result<Keypair, Box<dyn error::Error>> {
    let keypair: Keypair =
        encodable_key_from_seed_phrase(keypair_name, skip_validation, derivation_path, legacy)?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&keypair, "pubkey");
    }
    Ok(keypair)
}

/// Reads user input from stdin to retrieve a seed phrase and passphrase for ElGamal keypair
/// derivation.
///
/// Optionally skips validation of seed phrase. Optionally confirms recovered public key.
pub fn elgamal_keypair_from_seed_phrase(
    elgamal_keypair_name: &str,
    skip_validation: bool,
    confirm_pubkey: bool,
    derivation_path: Option<DerivationPath>,
    legacy: bool,
) -> Result<ElGamalKeypair, Box<dyn error::Error>> {
    let elgamal_keypair: ElGamalKeypair = encodable_key_from_seed_phrase(
        elgamal_keypair_name,
        skip_validation,
        derivation_path,
        legacy,
    )?;
    if confirm_pubkey {
        confirm_encodable_keypair_pubkey(&elgamal_keypair, "ElGamal pubkey");
    }
    Ok(elgamal_keypair)
}

/// Reads user input from stdin to retrieve a seed phrase and passphrase for an authenticated
/// encryption keypair derivation.
pub fn ae_key_from_seed_phrase(
    keypair_name: &str,
    skip_validation: bool,
    derivation_path: Option<DerivationPath>,
    legacy: bool,
) -> Result<AeKey, Box<dyn error::Error>> {
    encodable_key_from_seed_phrase(keypair_name, skip_validation, derivation_path, legacy)
}

fn encodable_key_from_seed_phrase<K: EncodableKey + SeedDerivable>(
    key_name: &str,
    skip_validation: bool,
    derivation_path: Option<DerivationPath>,
    legacy: bool,
) -> Result<K, Box<dyn error::Error>> {
    let seed_phrase = prompt_password(format!("[{key_name}] seed phrase: "))?;
    let seed_phrase = seed_phrase.trim();
    let passphrase_prompt = format!(
        "[{key_name}] If this seed phrase has an associated passphrase, enter it now. Otherwise, press ENTER to continue: ",
    );

    let key = if skip_validation {
        let passphrase = prompt_passphrase(&passphrase_prompt)?;
        if legacy {
            K::from_seed_phrase_and_passphrase(seed_phrase, &passphrase)?
        } else {
            let seed = generate_seed_from_seed_phrase_and_passphrase(seed_phrase, &passphrase);
            K::from_seed_and_derivation_path(&seed, derivation_path)?
        }
    } else {
        let sanitized = sanitize_seed_phrase(seed_phrase);
        let parse_language_fn = || {
            for language in &[
                Language::English,
                Language::ChineseSimplified,
                Language::ChineseTraditional,
                Language::Japanese,
                Language::Spanish,
                Language::Korean,
                Language::French,
                Language::Italian,
            ] {
                if let Ok(mnemonic) = Mnemonic::from_phrase(&sanitized, *language) {
                    return Ok(mnemonic);
                }
            }
            Err("Can't get mnemonic from seed phrases")
        };
        let mnemonic = parse_language_fn()?;
        let passphrase = prompt_passphrase(&passphrase_prompt)?;
        let seed = Seed::new(&mnemonic, &passphrase);
        if legacy {
            K::from_seed(seed.as_bytes())?
        } else {
            K::from_seed_and_derivation_path(seed.as_bytes(), derivation_path)?
        }
    };
    Ok(key)
}

fn sanitize_seed_phrase(seed_phrase: &str) -> String {
    seed_phrase
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::offline::OfflineArgs,
        clap::{Arg, Command},
        solana_remote_wallet::remote_wallet::initialize_wallet_manager,
        solana_sdk::{signer::keypair::write_keypair_file, system_instruction},
        tempfile::TempDir,
    };

    #[test]
    fn test_sanitize_seed_phrase() {
        let seed_phrase = " Mary   had\ta\u{2009}little  \n\t lamb";
        assert_eq!(
            "Mary had a little lamb".to_owned(),
            sanitize_seed_phrase(seed_phrase)
        );
    }

    #[test]
    fn test_signer_info_signers_for_message() {
        let source = Keypair::new();
        let fee_payer = Keypair::new();
        let nonsigner1 = Keypair::new();
        let nonsigner2 = Keypair::new();
        let recipient = Pubkey::new_unique();
        let message = Message::new(
            &[system_instruction::transfer(
                &source.pubkey(),
                &recipient,
                42,
            )],
            Some(&fee_payer.pubkey()),
        );
        let signers = vec![
            Box::new(fee_payer) as Box<dyn Signer>,
            Box::new(source) as Box<dyn Signer>,
            Box::new(nonsigner1) as Box<dyn Signer>,
            Box::new(nonsigner2) as Box<dyn Signer>,
        ];
        let signer_info = CliSignerInfo { signers };
        let msg_signers = signer_info.signers_for_message(&message);
        let signer_pubkeys = msg_signers.iter().map(|s| s.pubkey()).collect::<Vec<_>>();
        let expect = vec![
            signer_info.signers[0].pubkey(),
            signer_info.signers[1].pubkey(),
        ];
        assert_eq!(signer_pubkeys, expect);
    }

    #[test]
    fn signer_from_path_with_file() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let dir = dir.path();
        let keypair_path = dir.join("id.json");
        let keypair_path_str = keypair_path.to_str().expect("utf-8");

        let keypair = Keypair::new();
        write_keypair_file(&keypair, &keypair_path)?;

        let args = vec!["program", keypair_path_str];

        let clap_app = Command::new("my-program")
            .arg(
                Arg::new("keypair")
                    .required(true)
                    .help("The signing keypair"),
            )
            .offline_args();

        let clap_matches = clap_app.get_matches_from(args);
        let keypair_str: String = clap_matches.value_of_t_or_exit("keypair");

        let wallet_manager = initialize_wallet_manager()?;

        let signer = signer_from_path(
            &clap_matches,
            &keypair_str,
            "signer",
            &mut Some(wallet_manager),
        )?;

        assert_eq!(keypair.pubkey(), signer.pubkey());

        Ok(())
    }
}
