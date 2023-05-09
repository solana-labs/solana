//! Used by validators to run events on exit.

use std::fmt;

#[derive(Default)]
pub struct Exit {
    exited: bool,
    exits: Vec<Box<dyn FnOnce() + Send + Sync>>,
}

impl Exit {
    pub fn register_exit(&mut self, exit: Box<dyn FnOnce() + Send + Sync>) {
        if self.exited {
            exit();
        } else {
            self.exits.push(exit);
        }
    }

    pub fn exit(&mut self) {
        self.exited = true;
        for exit in self.exits.drain(..) {
            exit();
        }
    }
}

impl fmt::Debug for Exit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} exits", self.exits.len())
    }
}
