use clap::ArgMatches;
use solana_sdk::{
    native_token::sol_to_lamports,
    pubkey::Pubkey,
    signature::{read_keypair, Keypair, KeypairUtil},
};

// Return parsed values from matches at `name`
pub fn values_of<T>(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<T>>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .values_of(name)
        .map(|xs| xs.map(|x| x.parse::<T>().unwrap()).collect())
}

// Return a parsed value from matches at `name`
pub fn value_of<T>(matches: &ArgMatches<'_>, name: &str) -> Option<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    if let Some(value) = matches.value_of(name) {
        value.parse::<T>().ok()
    } else {
        None
    }
}

// Return the keypair for an argument with filename `name` or None if not present.
pub fn keypair_of(matches: &ArgMatches<'_>, name: &str) -> Option<Keypair> {
    if let Some(value) = matches.value_of(name) {
        read_keypair(value).ok()
    } else {
        None
    }
}

// Return a pubkey for an argument that can itself be parsed into a pubkey,
// or is a filename that can be read as a keypair
pub fn pubkey_of(matches: &ArgMatches<'_>, name: &str) -> Option<Pubkey> {
    value_of(matches, name).or_else(|| keypair_of(matches, name).map(|keypair| keypair.pubkey()))
}

pub fn amount_of(matches: &ArgMatches<'_>, name: &str, unit: &str) -> Option<u64> {
    if matches.value_of(unit) == Some("lamports") {
        value_of(matches, name)
    } else {
        value_of(matches, name).map(sol_to_lamports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{App, Arg};
    use solana_sdk::signature::write_keypair;
    use std::fs;

    fn app<'ab, 'v>() -> App<'ab, 'v> {
        App::new("test")
            .arg(
                Arg::with_name("multiple")
                    .long("multiple")
                    .takes_value(true)
                    .multiple(true),
            )
            .arg(Arg::with_name("single").takes_value(true).long("single"))
    }

    fn tmp_file_path(name: &str, pubkey: &Pubkey) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());

        format!("{}/tmp/{}-{}", out_dir, name, pubkey.to_string())
    }

    #[test]
    fn test_values_of() {
        let matches =
            app()
                .clone()
                .get_matches_from(vec!["test", "--multiple", "50", "--multiple", "39"]);
        assert_eq!(values_of(&matches, "multiple"), Some(vec![50, 39]));
        assert_eq!(values_of::<u64>(&matches, "single"), None);

        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let matches = app().clone().get_matches_from(vec![
            "test",
            "--multiple",
            &pubkey0.to_string(),
            "--multiple",
            &pubkey1.to_string(),
        ]);
        assert_eq!(
            values_of(&matches, "multiple"),
            Some(vec![pubkey0, pubkey1])
        );
    }

    #[test]
    fn test_value_of() {
        let matches = app()
            .clone()
            .get_matches_from(vec!["test", "--single", "50"]);
        assert_eq!(value_of(&matches, "single"), Some(50));
        assert_eq!(value_of::<u64>(&matches, "multiple"), None);

        let pubkey = Pubkey::new_rand();
        let matches = app()
            .clone()
            .get_matches_from(vec!["test", "--single", &pubkey.to_string()]);
        assert_eq!(value_of(&matches, "single"), Some(pubkey));
    }

    #[test]
    fn test_keypair_of() {
        let keypair = Keypair::new();
        let outfile = tmp_file_path("test_gen_keypair_file.json", &keypair.pubkey());
        let _ = write_keypair(&keypair, &outfile).unwrap();

        let matches = app()
            .clone()
            .get_matches_from(vec!["test", "--single", &outfile]);
        assert_eq!(keypair_of(&matches, "single"), Some(keypair));
        assert_eq!(keypair_of(&matches, "multiple"), None);

        let matches =
            app()
                .clone()
                .get_matches_from(vec!["test", "--single", "random_keypair_file.json"]);
        assert_eq!(keypair_of(&matches, "single"), None);

        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_pubkey_of() {
        let keypair = Keypair::new();
        let outfile = tmp_file_path("test_gen_keypair_file.json", &keypair.pubkey());
        let _ = write_keypair(&keypair, &outfile).unwrap();

        let matches = app()
            .clone()
            .get_matches_from(vec!["test", "--single", &outfile]);
        assert_eq!(pubkey_of(&matches, "single"), Some(keypair.pubkey()));
        assert_eq!(pubkey_of(&matches, "multiple"), None);

        let matches =
            app()
                .clone()
                .get_matches_from(vec!["test", "--single", &keypair.pubkey().to_string()]);
        assert_eq!(pubkey_of(&matches, "single"), Some(keypair.pubkey()));

        let matches =
            app()
                .clone()
                .get_matches_from(vec!["test", "--single", "random_keypair_file.json"]);
        assert_eq!(pubkey_of(&matches, "single"), None);

        fs::remove_file(&outfile).unwrap();
    }
}
