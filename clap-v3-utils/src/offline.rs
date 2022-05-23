use {
    crate::{input_validators::*, ArgConstant},
    clap::{Arg, Command},
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
    help: "Provide a public-key/signature pair for the transaction",
};

pub const DUMP_TRANSACTION_MESSAGE: ArgConstant<'static> = ArgConstant {
    name: "dump_transaction_message",
    long: "dump-transaction-message",
    help: "Display the base64 encoded binary transaction message in sign-only mode",
};

pub fn blockhash_arg<'a>() -> Arg<'a> {
    Arg::new(BLOCKHASH_ARG.name)
        .long(BLOCKHASH_ARG.long)
        .takes_value(true)
        .value_name("BLOCKHASH")
        .validator(is_hash)
        .help(BLOCKHASH_ARG.help)
}

pub fn sign_only_arg<'a>() -> Arg<'a> {
    Arg::new(SIGN_ONLY_ARG.name)
        .long(SIGN_ONLY_ARG.long)
        .takes_value(false)
        .requires(BLOCKHASH_ARG.name)
        .help(SIGN_ONLY_ARG.help)
}

fn signer_arg<'a>() -> Arg<'a> {
    Arg::new(SIGNER_ARG.name)
        .long(SIGNER_ARG.long)
        .takes_value(true)
        .value_name("PUBKEY=SIGNATURE")
        .validator(is_pubkey_sig)
        .requires(BLOCKHASH_ARG.name)
        .multiple_occurrences(true)
        .multiple_values(true)
        .help(SIGNER_ARG.help)
}

pub fn dump_transaction_message<'a>() -> Arg<'a> {
    Arg::new(DUMP_TRANSACTION_MESSAGE.name)
        .long(DUMP_TRANSACTION_MESSAGE.long)
        .takes_value(false)
        .requires(SIGN_ONLY_ARG.name)
        .help(DUMP_TRANSACTION_MESSAGE.help)
}

pub trait ArgsConfig {
    fn blockhash_arg<'a>(&self, arg: Arg<'a>) -> Arg<'a> {
        arg
    }
    fn sign_only_arg<'a>(&self, arg: Arg<'a>) -> Arg<'a> {
        arg
    }
    fn signer_arg<'a>(&self, arg: Arg<'a>) -> Arg<'a> {
        arg
    }
    fn dump_transaction_message_arg<'a>(&self, arg: Arg<'a>) -> Arg<'a> {
        arg
    }
}

pub trait OfflineArgs {
    fn offline_args(self) -> Self;
    fn offline_args_config(self, config: &dyn ArgsConfig) -> Self;
}

impl OfflineArgs for Command<'_> {
    fn offline_args_config(self, config: &dyn ArgsConfig) -> Self {
        self.arg(config.blockhash_arg(blockhash_arg()))
            .arg(config.sign_only_arg(sign_only_arg()))
            .arg(config.signer_arg(signer_arg()))
            .arg(config.dump_transaction_message_arg(dump_transaction_message()))
    }
    fn offline_args(self) -> Self {
        struct NullArgsConfig {}
        impl ArgsConfig for NullArgsConfig {}
        self.offline_args_config(&NullArgsConfig {})
    }
}
