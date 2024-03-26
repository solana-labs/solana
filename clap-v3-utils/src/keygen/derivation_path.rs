use {
    clap::{Arg, ArgMatches},
    solana_sdk::derivation_path::DerivationPath,
    std::error,
};

pub const DEFAULT_DERIVATION_PATH: &str = "m/44'/501'/0'/0'";

pub fn derivation_path_arg<'a>() -> Arg<'a> {
    Arg::new("derivation_path")
        .long("derivation-path")
        .value_name("DERIVATION_PATH")
        .takes_value(true)
        .min_values(0)
        .max_values(1)
        .help("Derivation path. All indexes will be promoted to hardened. \
            If arg is not presented then derivation path will not be used. \
            If arg is presented with empty DERIVATION_PATH value then m/44'/501'/0'/0' will be used."
        )
}

pub fn acquire_derivation_path(
    matches: &ArgMatches,
) -> Result<Option<DerivationPath>, Box<dyn error::Error>> {
    if matches.try_contains_id("derivation_path")? {
        Ok(Some(DerivationPath::from_absolute_path_str(
            matches
                .try_get_one::<String>("derivation_path")?
                .map(|path| path.as_str())
                .unwrap_or(DEFAULT_DERIVATION_PATH),
        )?))
    } else {
        Ok(None)
    }
}
