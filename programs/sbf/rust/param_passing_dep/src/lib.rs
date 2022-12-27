//! Example Rust-based SBF program tests loop iteration

extern crate solana_program;

#[derive(Debug)]
pub struct Data<'a> {
    pub twentyone: u64,
    pub twentytwo: u64,
    pub twentythree: u64,
    pub twentyfour: u64,
    pub twentyfive: u32,
    pub array: &'a [u8],
}

#[derive(PartialEq, Debug)]
pub struct TestDep {
    pub thirty: u32,
}
impl<'a> TestDep {
    pub fn new(data: &Data<'a>, _one: u64, _two: u64, _three: u64, _four: u64, five: u64) -> Self {
        Self {
            thirty: data.twentyfive + five as u32,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_dep() {
        let array = [0xA, 0xB, 0xC, 0xD, 0xE, 0xF];
        let data = Data {
            twentyone: 21u64,
            twentytwo: 22u64,
            twentythree: 23u64,
            twentyfour: 24u64,
            twentyfive: 25u32,
            array: &array,
        };
        assert_eq!(TestDep { thirty: 30 }, TestDep::new(&data, 1, 2, 3, 4, 5));
    }
}
