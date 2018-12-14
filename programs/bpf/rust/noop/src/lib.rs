extern crate rbpf;

use std::mem::transmute;

#[no_mangle]
#[link_section = ".text,entrypoint"] // TODO platform independent needed
pub extern "C" fn entrypoint(_raw: *mut u8) {
    let bpf_func_trace_printk = unsafe {
        transmute::<u64, extern "C" fn(u64, u64, u64, u64, u64)>(u64::from(
            rbpf::helpers::BPF_TRACE_PRINTK_IDX,
        ))
    };

    bpf_func_trace_printk(0, 0, 1, 2, 3);
}
