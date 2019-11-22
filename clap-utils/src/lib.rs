#[macro_export]
macro_rules! version {
    () => {
        &*format!(
            "{}{}",
            env!("CARGO_PKG_VERSION"),
            if option_env!("CI_TAG").unwrap_or("").is_empty() {
                format!(
                    " [channel={} commit={}]",
                    option_env!("CHANNEL").unwrap_or("unknown"),
                    option_env!("CI_COMMIT").unwrap_or("unknown"),
                )
            } else {
                "".to_string()
            },
        )
    };
}

pub mod input_parsers;
pub mod input_validators;
pub mod keypair;
