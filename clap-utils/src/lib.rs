use thiserror::Error;

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

pub struct ArgConstant<'a> {
    pub long: &'a str,
    pub name: &'a str,
    pub help: &'a str,
}

/// Error type for forwarding Errors out of `main()` of a `clap` app
/// and still using the `Display` formatter
#[derive(Error)]
#[error("{0}")]
pub struct DisplayError(Box<dyn std::error::Error>);
impl DisplayError {
    pub fn new_as_boxed(inner: Box<dyn std::error::Error>) -> Box<Self> {
        DisplayError(inner).into()
    }
}

impl std::fmt::Debug for DisplayError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

pub mod input_parsers;
pub mod input_validators;
pub mod keypair;
pub mod offline;
