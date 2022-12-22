use {super::*, crate::declare_syscall};

fn mem_op_consume(invoke_context: &mut InvokeContext, n: u64) -> Result<(), Error> {
    let compute_budget = invoke_context.get_compute_budget();
    let cost = compute_budget
        .mem_op_base_cost
        .max(n.saturating_div(compute_budget.cpi_bytes_per_unit));
    consume_compute_meter(invoke_context, cost)
}

declare_syscall!(
    /// memcpy
    SyscallMemcpy,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        dst_addr: u64,
        src_addr: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        if !is_nonoverlapping(src_addr, n, dst_addr, n) {
            return Err(SyscallError::CopyOverlapping.into());
        }

        let dst_ptr = translate_slice_mut::<u8>(
            memory_mapping,
            dst_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?
        .as_mut_ptr();
        let src_ptr = translate_slice::<u8>(
            memory_mapping,
            src_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?
        .as_ptr();
        if !is_nonoverlapping(src_ptr as usize, n as usize, dst_ptr as usize, n as usize) {
            unsafe {
                std::ptr::copy(src_ptr, dst_ptr, n as usize);
            }
        } else {
            unsafe {
                std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, n as usize);
            }
        }
        Ok(0)
    }
);

declare_syscall!(
    /// memmove
    SyscallMemmove,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        dst_addr: u64,
        src_addr: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        let dst = translate_slice_mut::<u8>(
            memory_mapping,
            dst_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let src = translate_slice::<u8>(
            memory_mapping,
            src_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        unsafe {
            std::ptr::copy(src.as_ptr(), dst.as_mut_ptr(), n as usize);
        }
        Ok(0)
    }
);

declare_syscall!(
    /// memcmp
    SyscallMemcmp,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        s1_addr: u64,
        s2_addr: u64,
        n: u64,
        cmp_result_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        let s1 = translate_slice::<u8>(
            memory_mapping,
            s1_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let s2 = translate_slice::<u8>(
            memory_mapping,
            s2_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        let cmp_result = translate_type_mut::<i32>(
            memory_mapping,
            cmp_result_addr,
            invoke_context.get_check_aligned(),
        )?;

        debug_assert_eq!(s1.len(), n as usize);
        debug_assert_eq!(s2.len(), n as usize);
        // Safety:
        // memcmp is marked unsafe since it assumes that the inputs are at least
        // `n` bytes long. `s1` and `s2` are guaranteed to be exactly `n` bytes
        // long because `translate_slice` would have failed otherwise.
        *cmp_result = unsafe { memcmp(s1, s2, n as usize) };
        Ok(0)
    }
);

declare_syscall!(
    /// memset
    SyscallMemset,
    fn inner_call(
        invoke_context: &mut InvokeContext,
        s_addr: u64,
        c: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        let s = translate_slice_mut::<u8>(
            memory_mapping,
            s_addr,
            n,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )?;
        for val in s.iter_mut().take(n as usize) {
            *val = c as u8;
        }
        Ok(0)
    }
);

// Marked unsafe since it assumes that the slices are at least `n` bytes long.
#[allow(clippy::indexing_slicing)]
unsafe fn memcmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let a = *s1.get_unchecked(i);
        let b = *s2.get_unchecked(i);
        if a != b {
            return (a as i32).saturating_sub(b as i32);
        };
    }

    0
}
