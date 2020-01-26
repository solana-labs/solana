use clap::{App, Arg};
use solana_clap_utils::{
    input_validators::{is_hash, is_pubkey_sig},
    ArgConstant,
};

pub const BLOCKHASH_ARG: ArgConstant<'static> = ArgConstant {
    name: "blockhash",
    long: "blockhash",
    help: "Use the supplied blockhash",
};

pub const SIGN_ONLY_ARG: ArgConstant<'static> = ArgConstant {
    name: "sign_only",
    long: "sign-only",
    help: "Sign the transaction offline",
};

pub const SIGNER_ARG: ArgConstant<'static> = ArgConstant {
    name: "signer",
    long: "signer",
    help: "Provid a public-key/signature pair for the transaction",
};

fn blockhash_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(BLOCKHASH_ARG.name)
        .long(BLOCKHASH_ARG.long)
        .takes_value(true)
        .value_name("BLOCKHASH")
        .validator(is_hash)
        .help(BLOCKHASH_ARG.help)
}

fn sign_only_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(SIGN_ONLY_ARG.name)
        .long(SIGN_ONLY_ARG.long)
        .takes_value(false)
        .help(SIGN_ONLY_ARG.help)
}

fn signer_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(SIGNER_ARG.name)
        .long(SIGNER_ARG.long)
        .takes_value(true)
        .value_name("BASE58_PUBKEY=BASE58_SIG")
        .validator(is_pubkey_sig)
        .multiple(true)
        .help(SIGNER_ARG.help)
}

pub trait OfflineArgs {
    fn offline_args(self) -> Self;
}

impl OfflineArgs for App<'_, '_> {
    fn offline_args(self) -> Self {
        self.arg(blockhash_arg())
            .arg(sign_only_arg())
            .arg(signer_arg())
    }
}
