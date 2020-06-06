use std::cell::RefCell;

#[derive(Default)]
pub struct LogCollector {
    messages: RefCell<Vec<String>>,
}

impl LogCollector {
    pub fn log(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string())
    }

    pub fn output(self) -> Vec<String> {
        self.messages.into_inner()
    }
}
