extern crate libc;

#[cfg(feature = "cuda")]
#[link(name = "nvToolsExt")]
extern "C" {
    fn nvtxRangeStartA(name: *const libc::c_char) -> u64;
    fn nvtxRangeEnd(id: u64);
}

#[cfg(feature = "cuda")]
pub fn nv_range_start(name: String) -> u64{
    use std::ffi::CString;

    unsafe {
        let cname = CString::new(name).unwrap();
        nvtxRangeStartA(cname.as_ptr())
    }
}

#[cfg(feature = "cuda")]
pub fn nv_range_end(id: u64) {
    unsafe {
        nvtxRangeEnd(id);
    }
}
