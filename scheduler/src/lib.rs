#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SchedulingMode {
    BlockVerification, //BlockProduction,
}

pub trait WithSchedulingMode {
    fn mode(&self) -> SchedulingMode;
}
