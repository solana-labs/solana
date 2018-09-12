use std::thread::Result;

pub trait Service {
    type JoinReturnType;

    fn join(self) -> Result<Self::JoinReturnType>;
}
