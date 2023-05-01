//! Debug-formatting of account data.

use std::{cmp, fmt};

pub(crate) const MAX_DEBUG_ACCOUNT_DATA: usize = 64;

/// Format data as hex.
///
/// If `data`'s length is greater than 0, add a field called "data" to `f`. The
/// first 64 bytes of `data` is displayed; bytes after that are ignored.
pub fn debug_account_data(data: &[u8], f: &mut fmt::DebugStruct<'_, '_>) {
    //let data_len = cmp::min(MAX_DEBUG_ACCOUNT_DATA, data.len());
    let data_len = data.len();
    if data_len > 0 {
        f.field("data", &Hex(&data[..data_len]));
    }
}

pub(crate) struct Hex<'a>(pub(crate) &'a [u8]);
impl fmt::Debug for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut i = 0;
        write!(f, "\n")?;
        for c in self.0.chunks(16) {
            write!(f, "{i:07x} ")?;
            for &byte in c {
                write!(f, "{byte:02x} ")?;
            }
            write!(f, "\n")?;
            i += 1;
        }
        Ok(())
    }
}

#[test]
fn test_debug_print() {
    let a: [u8; 19] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
    ];

    println!("{:?}", Hex(&a));
}
