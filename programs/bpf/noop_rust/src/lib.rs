#[no_mangle]
#[link_section = ".text,entrypoint"] // TODO platform independent needed
pub extern "C" fn entrypoint(_raw: *mut u8) {}
