use {super::*, crate::declare_syscall};

fn mem_op_consume<'a, 'b>(
    invoke_context: &Ref<&'a mut InvokeContext<'b>>,
    n: u64,
) -> Result<(), EbpfError<BpfError>> {
    let compute_budget = invoke_context.get_compute_budget();
    let cost = compute_budget
        .mem_op_base_cost
        .max(n.saturating_div(compute_budget.cpi_bytes_per_unit));
    invoke_context.get_compute_meter().consume(cost)
}

declare_syscall!(
    /// memcpy
    SyscallMemcpy,
    fn call(
        &mut self,
        dst_addr: u64,
        src_addr: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        let invoke_context = question_mark!(
            self.invoke_context
                .try_borrow()
                .map_err(|_| SyscallError::InvokeContextBorrowFailed),
            result
        );
        question_mark!(mem_op_consume(&invoke_context, n), result);

        let do_check_physical_overlapping = invoke_context
            .feature_set
            .is_active(&check_physical_overlapping::id());

        if !is_nonoverlapping(src_addr, dst_addr, n) {
            *result = Err(SyscallError::CopyOverlapping.into());
            return;
        }

        let dst_ptr = question_mark!(
            translate_slice_mut::<u8>(
                memory_mapping,
                dst_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size()
            ),
            result
        )
        .as_mut_ptr();
        let src_ptr = question_mark!(
            translate_slice::<u8>(
                memory_mapping,
                src_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size()
            ),
            result
        )
        .as_ptr();
        if do_check_physical_overlapping
            && !is_nonoverlapping(src_ptr as usize, dst_ptr as usize, n as usize)
        {
            unsafe {
                std::ptr::copy(src_ptr, dst_ptr, n as usize);
            }
        } else {
            unsafe {
                std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, n as usize);
            }
        }
        *result = Ok(0);
    }
);

declare_syscall!(
    /// memmove
    SyscallMemmove,
    fn call(
        &mut self,
        dst_addr: u64,
        src_addr: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        let invoke_context = question_mark!(
            self.invoke_context
                .try_borrow()
                .map_err(|_| SyscallError::InvokeContextBorrowFailed),
            result
        );
        question_mark!(mem_op_consume(&invoke_context, n), result);

        let dst = question_mark!(
            translate_slice_mut::<u8>(
                memory_mapping,
                dst_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size()
            ),
            result
        );
        let src = question_mark!(
            translate_slice::<u8>(
                memory_mapping,
                src_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size()
            ),
            result
        );
        unsafe {
            std::ptr::copy(src.as_ptr(), dst.as_mut_ptr(), n as usize);
        }
        *result = Ok(0);
    }
);

declare_syscall!(
    /// memcmp
    SyscallMemcmp,
    fn call(
        &mut self,
        s1_addr: u64,
        s2_addr: u64,
        n: u64,
        cmp_result_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        let invoke_context = question_mark!(
            self.invoke_context
                .try_borrow()
                .map_err(|_| SyscallError::InvokeContextBorrowFailed),
            result
        );
        question_mark!(mem_op_consume(&invoke_context, n), result);

        let s1 = question_mark!(
            translate_slice::<u8>(
                memory_mapping,
                s1_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            ),
            result
        );
        let s2 = question_mark!(
            translate_slice::<u8>(
                memory_mapping,
                s2_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            ),
            result
        );
        let cmp_result = question_mark!(
            translate_type_mut::<i32>(
                memory_mapping,
                cmp_result_addr,
                invoke_context.get_check_aligned()
            ),
            result
        );
        let mut i = 0;
        while i < n as usize {
            let a = *question_mark!(s1.get(i).ok_or(SyscallError::InvalidLength,), result);
            let b = *question_mark!(s2.get(i).ok_or(SyscallError::InvalidLength,), result);
            if a != b {
                *cmp_result = if invoke_context
                    .feature_set
                    .is_active(&syscall_saturated_math::id())
                {
                    (a as i32).saturating_sub(b as i32)
                } else {
                    #[allow(clippy::integer_arithmetic)]
                    {
                        a as i32 - b as i32
                    }
                };
                *result = Ok(0);
                return;
            };
            i = i.saturating_add(1);
        }
        *cmp_result = 0;
        *result = Ok(0);
    }
);

declare_syscall!(
    /// memset
    SyscallMemset,
    fn call(
        &mut self,
        s_addr: u64,
        c: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
        result: &mut Result<u64, EbpfError<BpfError>>,
    ) {
        let invoke_context = question_mark!(
            self.invoke_context
                .try_borrow()
                .map_err(|_| SyscallError::InvokeContextBorrowFailed),
            result
        );
        question_mark!(mem_op_consume(&invoke_context, n), result);

        let s = question_mark!(
            translate_slice_mut::<u8>(
                memory_mapping,
                s_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            ),
            result
        );
        for val in s.iter_mut().take(n as usize) {
            *val = c as u8;
        }
        *result = Ok(0);
    }
);
