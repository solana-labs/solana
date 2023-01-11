//! The `logger` module configures `env_logger`

use {
    env_logger::{fmt::Formatter, Builder, Env, Logger, Target},
    lazy_static::lazy_static,
    log::Record,
    std::{
        io::{self, Write},
        sync::{Arc, RwLock},
        thread,
    },
};

lazy_static! {
    static ref LOGGER: Arc<RwLock<Logger>> = Arc::new(RwLock::new(Logger::from_default_env()));
}

struct LoggerShim {}

impl log::Log for LoggerShim {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        LOGGER.read().unwrap().enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        LOGGER.read().unwrap().log(record);
    }

    fn flush(&self) {}
}

fn write_thread_name_and_id(fmt: &mut impl Write) -> io::Result<()> {
    let current = &thread::current();

    // Remove `ThreadId()` wrapping, leaving just the actual ID value.  This makes our log messages
    // a bit shorter.
    //
    // TODO Switch to just calling `as_u64()`, when it is stabilized:
    //
    //   https://github.com/rust-lang/rust/issues/67939

    // Preallocate 10 characters for `ThreadId()`, and 10 more for the number itself.
    let mut id_buf = Vec::<u8>::with_capacity(20);
    write!(&mut id_buf, "{:?}", current.id())?;
    let id_part = id_buf
        .strip_prefix(b"ThreadId(")
        .and_then(|v| v.strip_suffix(b")"))
        .unwrap_or_else(|| &id_buf[..]);

    fmt.write_all(b" ")?;
    fmt.write_all(id_part)?;

    if let Some(name) = current.name() {
        write!(fmt, " {name}")?;
    }

    Ok(())
}

/// Formats [`log::Record`] entries.
///
/// Produces result similar to the one of `env_logger::Builder().format_timestamp_nanos()`, but
/// inserts the current thread ID into the generated message.
fn format_record(fmt: &mut Formatter, record: &Record) -> io::Result<()> {
    write!(fmt, "[{}", fmt.timestamp_nanos())?;

    write_thread_name_and_id(fmt)?;

    write!(
        fmt,
        " {:<5}",
        fmt.default_styled_level(record.level()),
        // `module_path` is disabled in the default [`env_logger::Builder`] config.
        // fmt.module_path(),
    )?;

    match record.target() {
        "" => write!(fmt, " ]")?,
        target => write!(fmt, " {target}]")?,
    }

    writeln!(fmt, " {}", record.args())
}

fn replace_logger(logger: Logger) {
    log::set_max_level(logger.filter());
    *LOGGER.write().unwrap() = logger;
    let _ = log::set_boxed_logger(Box::new(LoggerShim {}));
}

// Configures logging with a specific filter overriding RUST_LOG.  _RUST_LOG is used instead
// so if set it takes precedence.
// May be called at any time to re-configure the log filter
pub fn setup_with(filter: &str) {
    let logger = Builder::from_env(Env::new().filter_or("_RUST_LOG", filter))
        .format(format_record)
        .build();
    replace_logger(logger);
}

// Configures logging with a default filter if RUST_LOG is not set
pub fn setup_with_default(filter: &str) {
    let logger = Builder::from_env(Env::new().default_filter_or(filter))
        .format(format_record)
        .build();
    replace_logger(logger);
}

// Configures logging with the default filter "error" if RUST_LOG is not set
pub fn setup() {
    setup_with_default("error");
}

// Configures file logging with a default filter if RUST_LOG is not set
pub fn setup_file_with_default(logfile: &str, filter: &str) {
    use std::fs::OpenOptions;
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(logfile)
        .unwrap();
    let logger = Builder::from_env(Env::new().default_filter_or(filter))
        .format(format_record)
        .target(Target::Pipe(Box::new(file)))
        .build();
    replace_logger(logger);
}
