use std::cell::RefCell;

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
