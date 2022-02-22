use log::{Level, LevelFilter, Log, Metadata, Record};

#[derive(Default)]
pub(crate) struct Logger {
    pub(crate) verbose: bool,
}

impl Logger {
    pub(crate) fn new(verbose: bool) -> Self {
        log::set_max_level(LevelFilter::Debug);
        Self { verbose }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let target = if self.verbose {
            Level::Debug
        } else {
            Level::Info
        };
        metadata.level() <= target
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}
