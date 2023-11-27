use {
    super::*,
    solana_rbpf::{error::EbpfError, memory_region::MemoryRegion},
    std::slice,
};

fn mem_op_consume(invoke_context: &mut InvokeContext, n: u64) -> Result<(), Error> {
    let compute_budget = invoke_context.get_compute_budget();
    let cost = compute_budget.mem_op_base_cost.max(
        n.checked_div(compute_budget.cpi_bytes_per_unit)
            .unwrap_or(u64::MAX),
    );
    consume_compute_meter(invoke_context, cost)
}

declare_builtin_function!(
    /// memcpy
    SyscallMemcpy,
    fn rust(
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

        // host addresses can overlap so we always invoke memmove
        memmove(invoke_context, dst_addr, src_addr, n, memory_mapping)
    }
);

declare_builtin_function!(
    /// memmove
    SyscallMemmove,
    fn rust(
        invoke_context: &mut InvokeContext,
        dst_addr: u64,
        src_addr: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        memmove(invoke_context, dst_addr, src_addr, n, memory_mapping)
    }
);

declare_builtin_function!(
    /// memcmp
    SyscallMemcmp,
    fn rust(
        invoke_context: &mut InvokeContext,
        s1_addr: u64,
        s2_addr: u64,
        n: u64,
        cmp_result_addr: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        if invoke_context
            .feature_set
            .is_active(&feature_set::bpf_account_data_direct_mapping::id())
        {
            let cmp_result = translate_type_mut::<i32>(
                memory_mapping,
                cmp_result_addr,
                invoke_context.get_check_aligned(),
            )?;
            *cmp_result = memcmp_non_contiguous(s1_addr, s2_addr, n, memory_mapping)?;
        } else {
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
        }

        Ok(0)
    }
);

declare_builtin_function!(
    /// memset
    SyscallMemset,
    fn rust(
        invoke_context: &mut InvokeContext,
        dst_addr: u64,
        c: u64,
        n: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        mem_op_consume(invoke_context, n)?;

        if invoke_context
            .feature_set
            .is_active(&feature_set::bpf_account_data_direct_mapping::id())
        {
            memset_non_contiguous(dst_addr, c as u8, n, memory_mapping)
        } else {
            let s = translate_slice_mut::<u8>(
                memory_mapping,
                dst_addr,
                n,
                invoke_context.get_check_aligned(),
                invoke_context.get_check_size(),
            )?;
            s.fill(c as u8);
            Ok(0)
        }
    }
);

fn memmove(
    invoke_context: &mut InvokeContext,
    dst_addr: u64,
    src_addr: u64,
    n: u64,
    memory_mapping: &MemoryMapping,
) -> Result<u64, Error> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::bpf_account_data_direct_mapping::id())
    {
        memmove_non_contiguous(dst_addr, src_addr, n, memory_mapping)
    } else {
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

        unsafe { std::ptr::copy(src_ptr, dst_ptr, n as usize) };
        Ok(0)
    }
}

fn memmove_non_contiguous(
    dst_addr: u64,
    src_addr: u64,
    n: u64,
    memory_mapping: &MemoryMapping,
) -> Result<u64, Error> {
    let reverse = dst_addr.wrapping_sub(src_addr) < n;
    iter_memory_pair_chunks(
        AccessType::Load,
        src_addr,
        AccessType::Store,
        dst_addr,
        n,
        memory_mapping,
        reverse,
        |src_host_addr, dst_host_addr, chunk_len| {
            unsafe { std::ptr::copy(src_host_addr, dst_host_addr as *mut u8, chunk_len) };
            Ok(0)
        },
    )
}

// Marked unsafe since it assumes that the slices are at least `n` bytes long.
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

fn memcmp_non_contiguous(
    src_addr: u64,
    dst_addr: u64,
    n: u64,
    memory_mapping: &MemoryMapping,
) -> Result<i32, Error> {
    match iter_memory_pair_chunks(
        AccessType::Load,
        src_addr,
        AccessType::Load,
        dst_addr,
        n,
        memory_mapping,
        false,
        |s1_addr, s2_addr, chunk_len| {
            let res = unsafe {
                let s1 = slice::from_raw_parts(s1_addr, chunk_len);
                let s2 = slice::from_raw_parts(s2_addr, chunk_len);
                // Safety:
                // memcmp is marked unsafe since it assumes that s1 and s2 are exactly chunk_len
                // long. The whole point of iter_memory_pair_chunks is to find same length chunks
                // across two memory regions.
                memcmp(s1, s2, chunk_len)
            };
            if res != 0 {
                return Err(MemcmpError::Diff(res).into());
            }
            Ok(0)
        },
    ) {
        Ok(res) => Ok(res),
        Err(error) => match error.downcast_ref() {
            Some(MemcmpError::Diff(diff)) => Ok(*diff),
            _ => Err(error),
        },
    }
}

#[derive(Debug)]
enum MemcmpError {
    Diff(i32),
}

impl std::fmt::Display for MemcmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemcmpError::Diff(diff) => write!(f, "memcmp diff: {diff}"),
        }
    }
}

impl std::error::Error for MemcmpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MemcmpError::Diff(_) => None,
        }
    }
}

fn memset_non_contiguous(
    dst_addr: u64,
    c: u8,
    n: u64,
    memory_mapping: &MemoryMapping,
) -> Result<u64, Error> {
    let dst_chunk_iter = MemoryChunkIterator::new(memory_mapping, AccessType::Store, dst_addr, n)?;
    for item in dst_chunk_iter {
        let (dst_region, dst_vm_addr, dst_len) = item?;
        let dst_host_addr = Result::from(dst_region.vm_to_host(dst_vm_addr, dst_len as u64))?;
        unsafe { slice::from_raw_parts_mut(dst_host_addr as *mut u8, dst_len).fill(c) }
    }

    Ok(0)
}

fn iter_memory_pair_chunks<T, F>(
    src_access: AccessType,
    src_addr: u64,
    dst_access: AccessType,
    dst_addr: u64,
    n_bytes: u64,
    memory_mapping: &MemoryMapping,
    reverse: bool,
    mut fun: F,
) -> Result<T, Error>
where
    T: Default,
    F: FnMut(*const u8, *const u8, usize) -> Result<T, Error>,
{
    let mut src_chunk_iter =
        MemoryChunkIterator::new(memory_mapping, src_access, src_addr, n_bytes)
            .map_err(EbpfError::from)?;
    let mut dst_chunk_iter =
        MemoryChunkIterator::new(memory_mapping, dst_access, dst_addr, n_bytes)
            .map_err(EbpfError::from)?;

    let mut src_chunk = None;
    let mut dst_chunk = None;

    macro_rules! memory_chunk {
        ($chunk_iter:ident, $chunk:ident) => {
            if let Some($chunk) = &mut $chunk {
                // Keep processing the current chunk
                $chunk
            } else {
                // This is either the first call or we've processed all the bytes in the current
                // chunk. Move to the next one.
                let chunk = match if reverse {
                    $chunk_iter.next_back()
                } else {
                    $chunk_iter.next()
                } {
                    Some(item) => item?,
                    None => break,
                };
                $chunk.insert(chunk)
            }
        };
    }

    loop {
        let (src_region, src_chunk_addr, src_remaining) = memory_chunk!(src_chunk_iter, src_chunk);
        let (dst_region, dst_chunk_addr, dst_remaining) = memory_chunk!(dst_chunk_iter, dst_chunk);

        // We always process same-length pairs
        let chunk_len = *src_remaining.min(dst_remaining);

        let (src_host_addr, dst_host_addr) = {
            let (src_addr, dst_addr) = if reverse {
                // When scanning backwards not only we want to scan regions from the end,
                // we want to process the memory within regions backwards as well.
                (
                    src_chunk_addr
                        .saturating_add(*src_remaining as u64)
                        .saturating_sub(chunk_len as u64),
                    dst_chunk_addr
                        .saturating_add(*dst_remaining as u64)
                        .saturating_sub(chunk_len as u64),
                )
            } else {
                (*src_chunk_addr, *dst_chunk_addr)
            };

            (
                Result::from(src_region.vm_to_host(src_addr, chunk_len as u64))?,
                Result::from(dst_region.vm_to_host(dst_addr, chunk_len as u64))?,
            )
        };

        fun(
            src_host_addr as *const u8,
            dst_host_addr as *const u8,
            chunk_len,
        )?;

        // Update how many bytes we have left to scan in each chunk
        *src_remaining = src_remaining.saturating_sub(chunk_len);
        *dst_remaining = dst_remaining.saturating_sub(chunk_len);

        if !reverse {
            // We've scanned `chunk_len` bytes so we move the vm address forward. In reverse
            // mode we don't do this since we make progress by decreasing src_len and
            // dst_len.
            *src_chunk_addr = src_chunk_addr.saturating_add(chunk_len as u64);
            *dst_chunk_addr = dst_chunk_addr.saturating_add(chunk_len as u64);
        }

        if *src_remaining == 0 {
            src_chunk = None;
        }

        if *dst_remaining == 0 {
            dst_chunk = None;
        }
    }

    Ok(T::default())
}

struct MemoryChunkIterator<'a> {
    memory_mapping: &'a MemoryMapping<'a>,
    access_type: AccessType,
    initial_vm_addr: u64,
    vm_addr_start: u64,
    // exclusive end index (start + len, so one past the last valid address)
    vm_addr_end: u64,
    len: u64,
}

impl<'a> MemoryChunkIterator<'a> {
    fn new(
        memory_mapping: &'a MemoryMapping,
        access_type: AccessType,
        vm_addr: u64,
        len: u64,
    ) -> Result<MemoryChunkIterator<'a>, EbpfError> {
        let vm_addr_end = vm_addr.checked_add(len).ok_or(EbpfError::AccessViolation(
            access_type,
            vm_addr,
            len,
            "unknown",
        ))?;
        Ok(MemoryChunkIterator {
            memory_mapping,
            access_type,
            initial_vm_addr: vm_addr,
            len,
            vm_addr_start: vm_addr,
            vm_addr_end,
        })
    }

    fn region(&mut self, vm_addr: u64) -> Result<&'a MemoryRegion, Error> {
        match self.memory_mapping.region(self.access_type, vm_addr) {
            Ok(region) => Ok(region),
            Err(error) => match error {
                EbpfError::AccessViolation(access_type, _vm_addr, _len, name) => Err(Box::new(
                    EbpfError::AccessViolation(access_type, self.initial_vm_addr, self.len, name),
                )),
                EbpfError::StackAccessViolation(access_type, _vm_addr, _len, frame) => {
                    Err(Box::new(EbpfError::StackAccessViolation(
                        access_type,
                        self.initial_vm_addr,
                        self.len,
                        frame,
                    )))
                }
                _ => Err(error.into()),
            },
        }
    }
}

impl<'a> Iterator for MemoryChunkIterator<'a> {
    type Item = Result<(&'a MemoryRegion, u64, usize), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.vm_addr_start == self.vm_addr_end {
            return None;
        }

        let region = match self.region(self.vm_addr_start) {
            Ok(region) => region,
            Err(e) => {
                self.vm_addr_start = self.vm_addr_end;
                return Some(Err(e));
            }
        };

        let vm_addr = self.vm_addr_start;

        let chunk_len = if region.vm_addr_end <= self.vm_addr_end {
            // consume the whole region
            let len = region.vm_addr_end.saturating_sub(self.vm_addr_start);
            self.vm_addr_start = region.vm_addr_end;
            len
        } else {
            // consume part of the region
            let len = self.vm_addr_end.saturating_sub(self.vm_addr_start);
            self.vm_addr_start = self.vm_addr_end;
            len
        };

        Some(Ok((region, vm_addr, chunk_len as usize)))
    }
}

impl<'a> DoubleEndedIterator for MemoryChunkIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.vm_addr_start == self.vm_addr_end {
            return None;
        }

        let region = match self.region(self.vm_addr_end.saturating_sub(1)) {
            Ok(region) => region,
            Err(e) => {
                self.vm_addr_start = self.vm_addr_end;
                return Some(Err(e));
            }
        };

        let chunk_len = if region.vm_addr >= self.vm_addr_start {
            // consume the whole region
            let len = self.vm_addr_end.saturating_sub(region.vm_addr);
            self.vm_addr_end = region.vm_addr;
            len
        } else {
            // consume part of the region
            let len = self.vm_addr_end.saturating_sub(self.vm_addr_start);
            self.vm_addr_end = self.vm_addr_start;
            len
        };

        Some(Ok((region, self.vm_addr_end, chunk_len as usize)))
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
#[allow(clippy::arithmetic_side_effects)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_rbpf::{ebpf::MM_PROGRAM_START, program::SBPFVersion},
        test_case::test_case,
    };

    fn to_chunk_vec<'a>(
        iter: impl Iterator<Item = Result<(&'a MemoryRegion, u64, usize), Error>>,
    ) -> Vec<(u64, usize)> {
        iter.flat_map(|res| res.map(|(_, vm_addr, len)| (vm_addr, len)))
            .collect::<Vec<_>>()
    }

    #[test]
    #[should_panic(expected = "AccessViolation")]
    fn test_memory_chunk_iterator_no_regions() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let memory_mapping = MemoryMapping::new(vec![], &config, &SBPFVersion::V2).unwrap();

        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, 0, 1).unwrap();
        src_chunk_iter.next().unwrap().unwrap();
    }

    #[test]
    #[should_panic(expected = "AccessViolation")]
    fn test_memory_chunk_iterator_new_out_of_bounds_upper() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let memory_mapping = MemoryMapping::new(vec![], &config, &SBPFVersion::V2).unwrap();

        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, u64::MAX, 1).unwrap();
        src_chunk_iter.next().unwrap().unwrap();
    }

    #[test]
    fn test_memory_chunk_iterator_out_of_bounds() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0xFF; 42];
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START)],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        // check oob at the lower bound on the first next()
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START - 1, 42)
                .unwrap();
        assert_matches!(
            src_chunk_iter.next().unwrap().unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 42, "unknown") if *addr == MM_PROGRAM_START - 1
        );

        // check oob at the upper bound. Since the memory mapping isn't empty,
        // this always happens on the second next().
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START, 43)
                .unwrap();
        assert!(src_chunk_iter.next().unwrap().is_ok());
        assert_matches!(
            src_chunk_iter.next().unwrap().unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 43, "program") if *addr == MM_PROGRAM_START
        );

        // check oob at the upper bound on the first next_back()
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START, 43)
                .unwrap()
                .rev();
        assert_matches!(
            src_chunk_iter.next().unwrap().unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 43, "program") if *addr == MM_PROGRAM_START
        );

        // check oob at the upper bound on the 2nd next_back()
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START - 1, 43)
                .unwrap()
                .rev();
        assert!(src_chunk_iter.next().unwrap().is_ok());
        assert_matches!(
            src_chunk_iter.next().unwrap().unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 43, "unknown") if *addr == MM_PROGRAM_START - 1
        );
    }

    #[test]
    fn test_memory_chunk_iterator_one() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0xFF; 42];
        let memory_mapping = MemoryMapping::new(
            vec![MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START)],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        // check lower bound
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START - 1, 1)
                .unwrap();
        assert!(src_chunk_iter.next().unwrap().is_err());

        // check upper bound
        let mut src_chunk_iter =
            MemoryChunkIterator::new(&memory_mapping, AccessType::Load, MM_PROGRAM_START + 42, 1)
                .unwrap();
        assert!(src_chunk_iter.next().unwrap().is_err());

        for (vm_addr, len) in [
            (MM_PROGRAM_START, 0),
            (MM_PROGRAM_START + 42, 0),
            (MM_PROGRAM_START, 1),
            (MM_PROGRAM_START, 42),
            (MM_PROGRAM_START + 41, 1),
        ] {
            for rev in [true, false] {
                let iter =
                    MemoryChunkIterator::new(&memory_mapping, AccessType::Load, vm_addr, len)
                        .unwrap();
                let res = if rev {
                    to_chunk_vec(iter.rev())
                } else {
                    to_chunk_vec(iter)
                };
                if len == 0 {
                    assert_eq!(res, &[]);
                } else {
                    assert_eq!(res, &[(vm_addr, len as usize)]);
                }
            }
        }
    }

    #[test]
    fn test_memory_chunk_iterator_two() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0x11; 8];
        let mem2 = vec![0x22; 4];
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START),
                MemoryRegion::new_readonly(&mem2, MM_PROGRAM_START + 8),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        for (vm_addr, len, mut expected) in [
            (MM_PROGRAM_START, 8, vec![(MM_PROGRAM_START, 8)]),
            (
                MM_PROGRAM_START + 7,
                2,
                vec![(MM_PROGRAM_START + 7, 1), (MM_PROGRAM_START + 8, 1)],
            ),
            (MM_PROGRAM_START + 8, 4, vec![(MM_PROGRAM_START + 8, 4)]),
        ] {
            for rev in [false, true] {
                let iter =
                    MemoryChunkIterator::new(&memory_mapping, AccessType::Load, vm_addr, len)
                        .unwrap();
                let res = if rev {
                    expected.reverse();
                    to_chunk_vec(iter.rev())
                } else {
                    to_chunk_vec(iter)
                };

                assert_eq!(res, expected);
            }
        }
    }

    #[test]
    fn test_iter_memory_pair_chunks_short() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0x11; 8];
        let mem2 = vec![0x22; 4];
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START),
                MemoryRegion::new_readonly(&mem2, MM_PROGRAM_START + 8),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        // dst is shorter than src
        assert_matches!(
            iter_memory_pair_chunks(
                AccessType::Load,
                MM_PROGRAM_START,
                AccessType::Load,
                MM_PROGRAM_START + 8,
                8,
                &memory_mapping,
                false,
                |_src, _dst, _len| Ok::<_, Error>(0),
            ).unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 8, "program") if *addr == MM_PROGRAM_START + 8
        );

        // src is shorter than dst
        assert_matches!(
            iter_memory_pair_chunks(
                AccessType::Load,
                MM_PROGRAM_START + 10,
                AccessType::Load,
                MM_PROGRAM_START + 2,
                3,
                &memory_mapping,
                false,
                |_src, _dst, _len| Ok::<_, Error>(0),
            ).unwrap_err().downcast_ref().unwrap(),
            EbpfError::AccessViolation(AccessType::Load, addr, 3, "program") if *addr == MM_PROGRAM_START + 10
        );
    }

    #[test]
    #[should_panic(expected = "AccessViolation(Store, 4294967296, 4")]
    fn test_memmove_non_contiguous_readonly() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0x11; 8];
        let mem2 = vec![0x22; 4];
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START),
                MemoryRegion::new_readonly(&mem2, MM_PROGRAM_START + 8),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        memmove_non_contiguous(MM_PROGRAM_START, MM_PROGRAM_START + 8, 4, &memory_mapping).unwrap();
    }

    #[test_case(&[], (0, 0, 0); "no regions")]
    #[test_case(&[10], (1, 10, 0); "single region 0 len")]
    #[test_case(&[10], (0, 5, 5); "single region no overlap")]
    #[test_case(&[10], (0, 0, 10) ; "single region complete overlap")]
    #[test_case(&[10], (2, 0, 5); "single region partial overlap start")]
    #[test_case(&[10], (0, 1, 6); "single region partial overlap middle")]
    #[test_case(&[10], (2, 5, 5); "single region partial overlap end")]
    #[test_case(&[3, 5], (0, 5, 2) ; "two regions no overlap, single source region")]
    #[test_case(&[4, 7], (0, 5, 5) ; "two regions no overlap, multiple source regions")]
    #[test_case(&[3, 8], (0, 0, 11) ; "two regions complete overlap")]
    #[test_case(&[2, 9], (3, 0, 5) ; "two regions partial overlap start")]
    #[test_case(&[3, 9], (1, 2, 5) ; "two regions partial overlap middle")]
    #[test_case(&[7, 3], (2, 6, 4) ; "two regions partial overlap end")]
    #[test_case(&[2, 6, 3, 4], (0, 10, 2) ; "many regions no overlap, single source region")]
    #[test_case(&[2, 1, 2, 5, 6], (2, 10, 4) ; "many regions no overlap, multiple source regions")]
    #[test_case(&[8, 1, 3, 6], (0, 0, 18) ; "many regions complete overlap")]
    #[test_case(&[7, 3, 1, 4, 5], (5, 0, 8) ; "many regions overlap start")]
    #[test_case(&[1, 5, 2, 9, 3], (5, 4, 8) ; "many regions overlap middle")]
    #[test_case(&[3, 9, 1, 1, 2, 1], (2, 9, 8) ; "many regions overlap end")]
    fn test_memmove_non_contiguous(
        regions: &[usize],
        (src_offset, dst_offset, len): (usize, usize, usize),
    ) {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let (mem, memory_mapping) = build_memory_mapping(regions, &config);

        // flatten the memory so we can memmove it with ptr::copy
        let mut expected_memory = flatten_memory(&mem);
        unsafe {
            std::ptr::copy(
                expected_memory.as_ptr().add(src_offset),
                expected_memory.as_mut_ptr().add(dst_offset),
                len,
            )
        };

        // do our memmove
        memmove_non_contiguous(
            MM_PROGRAM_START + dst_offset as u64,
            MM_PROGRAM_START + src_offset as u64,
            len as u64,
            &memory_mapping,
        )
        .unwrap();

        // flatten memory post our memmove
        let memory = flatten_memory(&mem);

        // compare libc's memmove with ours
        assert_eq!(expected_memory, memory);
    }

    #[test]
    #[should_panic(expected = "AccessViolation(Store, 4294967296, 9")]
    fn test_memset_non_contiguous_readonly() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mut mem1 = vec![0x11; 8];
        let mem2 = vec![0x22; 4];
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_writable(&mut mem1, MM_PROGRAM_START),
                MemoryRegion::new_readonly(&mem2, MM_PROGRAM_START + 8),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        assert_eq!(
            memset_non_contiguous(MM_PROGRAM_START, 0x33, 9, &memory_mapping).unwrap(),
            0
        );
    }

    #[test]
    fn test_memset_non_contiguous() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = vec![0x11; 1];
        let mut mem2 = vec![0x22; 2];
        let mut mem3 = vec![0x33; 3];
        let mut mem4 = vec![0x44; 4];
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START),
                MemoryRegion::new_writable(&mut mem2, MM_PROGRAM_START + 1),
                MemoryRegion::new_writable(&mut mem3, MM_PROGRAM_START + 3),
                MemoryRegion::new_writable(&mut mem4, MM_PROGRAM_START + 6),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        assert_eq!(
            memset_non_contiguous(MM_PROGRAM_START + 1, 0x55, 7, &memory_mapping).unwrap(),
            0
        );
        assert_eq!(&mem1, &[0x11]);
        assert_eq!(&mem2, &[0x55, 0x55]);
        assert_eq!(&mem3, &[0x55, 0x55, 0x55]);
        assert_eq!(&mem4, &[0x55, 0x55, 0x44, 0x44]);
    }

    #[test]
    fn test_memcmp_non_contiguous() {
        let config = Config {
            aligned_memory_mapping: false,
            ..Config::default()
        };
        let mem1 = b"foo".to_vec();
        let mem2 = b"barbad".to_vec();
        let mem3 = b"foobarbad".to_vec();
        let memory_mapping = MemoryMapping::new(
            vec![
                MemoryRegion::new_readonly(&mem1, MM_PROGRAM_START),
                MemoryRegion::new_readonly(&mem2, MM_PROGRAM_START + 3),
                MemoryRegion::new_readonly(&mem3, MM_PROGRAM_START + 9),
            ],
            &config,
            &SBPFVersion::V2,
        )
        .unwrap();

        // non contiguous src
        assert_eq!(
            memcmp_non_contiguous(MM_PROGRAM_START, MM_PROGRAM_START + 9, 9, &memory_mapping)
                .unwrap(),
            0
        );

        // non contiguous dst
        assert_eq!(
            memcmp_non_contiguous(
                MM_PROGRAM_START + 10,
                MM_PROGRAM_START + 1,
                8,
                &memory_mapping
            )
            .unwrap(),
            0
        );

        // diff
        assert_eq!(
            memcmp_non_contiguous(
                MM_PROGRAM_START + 1,
                MM_PROGRAM_START + 11,
                5,
                &memory_mapping
            )
            .unwrap(),
            unsafe { memcmp(b"oobar", b"obarb", 5) }
        );
    }

    fn build_memory_mapping<'a>(
        regions: &[usize],
        config: &'a Config,
    ) -> (Vec<Vec<u8>>, MemoryMapping<'a>) {
        let mut regs = vec![];
        let mut mem = Vec::new();
        let mut offset = 0;
        for (i, region_len) in regions.iter().enumerate() {
            mem.push(
                (0..*region_len)
                    .map(|x| (i * 10 + x) as u8)
                    .collect::<Vec<_>>(),
            );
            regs.push(MemoryRegion::new_writable(
                &mut mem[i],
                MM_PROGRAM_START + offset as u64,
            ));
            offset += *region_len;
        }

        let memory_mapping = MemoryMapping::new(regs, config, &SBPFVersion::V2).unwrap();

        (mem, memory_mapping)
    }

    fn flatten_memory(mem: &[Vec<u8>]) -> Vec<u8> {
        mem.iter().flatten().copied().collect()
    }
}
