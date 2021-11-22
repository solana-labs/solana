use std::{cell::RefCell, rc::Rc};
use log::*;

const LOG_MESSAGES_BYTES_LIMIT: usize = 10 * 1000;

#[derive(Default)]
struct LogCollectorInner {
    messages: Vec<String>,
    bytes_written: usize,
    limit_warning: bool,
}

#[derive(Default)]
pub struct LogCollector {
    inner: RefCell<LogCollectorInner>,
}

impl LogCollector {
    pub fn log(&self, message: &str) {
        let mut inner = self.inner.borrow_mut();
        let bytes_written = inner.bytes_written.saturating_add(message.len());
        if bytes_written >= LOG_MESSAGES_BYTES_LIMIT {
            if !inner.limit_warning {
                inner.limit_warning = true;
                inner.messages.push(String::from("Log truncated"));
            }
        } else {
            inner.bytes_written = bytes_written;
            inner.messages.push(message.to_string());
        }
    }
}

impl From<LogCollector> for Vec<String> {
    fn from(log_collector: LogCollector) -> Self {
        log_collector.inner.into_inner().messages
    }
}

/// Log messages
pub struct Logger {
    log_collector: Option<Rc<LogCollector>>,
}
impl Logger {
    /// Is logging enabled
    pub fn log_enabled(&self) -> bool {
        log_enabled!(log::Level::Info) || self.log_collector.is_some()
    }
    /// Log a message.
    ///
    /// Unless explicitly stated, log messages are not considered stable and may change in the
    /// future as necessary
    pub fn log(&self, message: &str) {
        debug!("{}", message);
        if let Some(log_collector) = &self.log_collector {
            log_collector.log(message);
        }
    }
    /// Construct a new one
    pub fn new_ref(log_collector: Option<Rc<LogCollector>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { log_collector }))
    }
}

/// Convenience macro to log a message with an `Rc<RefCell<Logger>>`
#[macro_export]
macro_rules! ic_logger_msg {
    ($logger:expr, $message:expr) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log($message);
            }
        }
    };
    ($logger:expr, $fmt:expr, $($arg:tt)*) => {
        if let Ok(logger) = $logger.try_borrow_mut() {
            if logger.log_enabled() {
                logger.log(&format!($fmt, $($arg)*));
            }
        }
    };
}

/// Convenience macro to log a message with an `InvokeContext`
#[macro_export]
macro_rules! ic_msg {
    ($invoke_context:expr, $message:expr) => {
        $crate::ic_logger_msg!($invoke_context.get_logger(), $message)
    };
    ($invoke_context:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::ic_logger_msg!($invoke_context.get_logger(), $fmt, $($arg)*)
    };
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_log_messages_bytes_limit() {
        let lc = LogCollector::default();

        for _i in 0..LOG_MESSAGES_BYTES_LIMIT * 2 {
            lc.log("x");
        }

        let logs: Vec<_> = lc.into();
        assert_eq!(logs.len(), LOG_MESSAGES_BYTES_LIMIT);
        for log in logs.iter().take(LOG_MESSAGES_BYTES_LIMIT - 1) {
            assert_eq!(*log, "x".to_string());
        }
        assert_eq!(logs.last(), Some(&"Log truncated".to_string()));
    }
}
