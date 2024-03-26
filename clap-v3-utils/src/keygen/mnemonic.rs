use {
    crate::{keypair::prompt_passphrase, ArgConstant},
    bip39::Language,
    clap::{Arg, ArgMatches},
    std::error,
};

pub const NO_PASSPHRASE: &str = "";

pub const WORD_COUNT_ARG: ArgConstant<'static> = ArgConstant {
    long: "word-count",
    name: "word_count",
    help: "Specify the number of words that will be present in the generated seed phrase",
};

pub const LANGUAGE_ARG: ArgConstant<'static> = ArgConstant {
    long: "language",
    name: "language",
    help: "Specify the mnemonic language that will be present in the generated seed phrase",
};

pub const NO_PASSPHRASE_ARG: ArgConstant<'static> = ArgConstant {
    long: "no-bip39-passphrase",
    name: "no_passphrase",
    help: "Do not prompt for a BIP39 passphrase",
};

pub fn word_count_arg<'a>() -> Arg<'a> {
    Arg::new(WORD_COUNT_ARG.name)
        .long(WORD_COUNT_ARG.long)
        .possible_values(["12", "15", "18", "21", "24"])
        .default_value("12")
        .value_name("NUMBER")
        .takes_value(true)
        .help(WORD_COUNT_ARG.help)
}

pub fn language_arg<'a>() -> Arg<'a> {
    Arg::new(LANGUAGE_ARG.name)
        .long(LANGUAGE_ARG.long)
        .possible_values([
            "english",
            "chinese-simplified",
            "chinese-traditional",
            "japanese",
            "spanish",
            "korean",
            "french",
            "italian",
        ])
        .default_value("english")
        .value_name("LANGUAGE")
        .takes_value(true)
        .help(LANGUAGE_ARG.help)
}

pub fn no_passphrase_arg<'a>() -> Arg<'a> {
    Arg::new(NO_PASSPHRASE_ARG.name)
        .long(NO_PASSPHRASE_ARG.long)
        .alias("no-passphrase")
        .help(NO_PASSPHRASE_ARG.help)
}

pub fn acquire_language(matches: &ArgMatches) -> Language {
    match matches
        .get_one::<String>(LANGUAGE_ARG.name)
        .unwrap()
        .as_str()
    {
        "english" => Language::English,
        "chinese-simplified" => Language::ChineseSimplified,
        "chinese-traditional" => Language::ChineseTraditional,
        "japanese" => Language::Japanese,
        "spanish" => Language::Spanish,
        "korean" => Language::Korean,
        "french" => Language::French,
        "italian" => Language::Italian,
        _ => unreachable!(),
    }
}

pub fn no_passphrase_and_message() -> (String, String) {
    (NO_PASSPHRASE.to_string(), "".to_string())
}

pub fn acquire_passphrase_and_message(
    matches: &ArgMatches,
) -> Result<(String, String), Box<dyn error::Error>> {
    if matches.try_contains_id(NO_PASSPHRASE_ARG.name)? {
        Ok(no_passphrase_and_message())
    } else {
        match prompt_passphrase(
            "\nFor added security, enter a BIP39 passphrase\n\
             \nNOTE! This passphrase improves security of the recovery seed phrase NOT the\n\
             keypair file itself, which is stored as insecure plain text\n\
             \nBIP39 Passphrase (empty for none): ",
        ) {
            Ok(passphrase) => {
                println!();
                Ok((passphrase, " and your BIP39 passphrase".to_string()))
            }
            Err(e) => Err(e),
        }
    }
}
