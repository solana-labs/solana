//! Debug-formatting of account data.

use std::{cmp, fmt};

pub(crate) const MAX_DEBUG_ACCOUNT_DATA: usize = 64;

/// Format data as hex.
///
/// If `data`'s length is greater than 0, add a field called "data" to `f`. The
/// first 64 bytes of `data` is displayed; bytes after that are ignored.
pub fn debug_account_data(data: &[u8], f: &mut fmt::DebugStruct<'_, '_>) {
    let data_len = cmp::min(MAX_DEBUG_ACCOUNT_DATA, data.len());
    if data_len > 0 {
        f.field("data", &Hex(&data[..data_len]));
    }
}

pub(crate) struct Hex<'a>(pub(crate) &'a [u8]);
impl fmt::Debug for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
