use {super::*, crate::declare_syscall};

fn mem_op_consume(invoke_context: &mut InvokeContext, n: u64) -> Result<(), EbpfError> {
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
    ) -> Result<u64, EbpfError> {
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
    ) -> Result<u64, EbpfError> {
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
    ) -> Result<u64, EbpfError> {
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
        let mut i = 0;
        while i < n as usize {
            let a = *s1.get(i).ok_or(SyscallError::InvalidLength)?;
            let b = *s2.get(i).ok_or(SyscallError::InvalidLength)?;
            if a != b {
                *cmp_result = (a as i32).saturating_sub(b as i32);
                return Ok(0);
            };
            i = i.saturating_add(1);
        }
        *cmp_result = 0;
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
    ) -> Result<u64, EbpfError> {
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
