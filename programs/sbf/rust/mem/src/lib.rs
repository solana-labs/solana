//! Test mem functions

#[cfg(any(not(feature = "no-entrypoint"), feature = "test-bpf"))]
pub mod entrypoint;

pub trait MemOps {
    fn memcpy(&self, dst: &mut [u8], src: &[u8], n: usize);
    /// # Safety
    unsafe fn memmove(&self, dst: *mut u8, src: *mut u8, n: usize);
    fn memset(&self, s: &mut [u8], c: u8, n: usize);
    fn memcmp(&self, s1: &[u8], s2: &[u8], n: usize) -> i32;
}

pub fn run_mem_tests<T: MemOps>(mem_ops: T) {
    // memcpy
    let src = &[1_u8; 18];
    let dst = &mut [0_u8; 1];
    mem_ops.memcpy(dst, src, 1);
    assert_eq!(&src[..1], dst);
    let dst = &mut [0_u8; 3];
    mem_ops.memcpy(dst, src, 3);
    assert_eq!(&src[..3], dst);
    let dst = &mut [0_u8; 8];
    mem_ops.memcpy(dst, src, 8);
    assert_eq!(&src[..8], dst);
    let dst = &mut [0_u8; 9];
    mem_ops.memcpy(dst, src, 9);
    assert_eq!(&src[..9], dst);
    let dst = &mut [0_u8; 16];
    mem_ops.memcpy(dst, src, 16);
    assert_eq!(&src[..16], dst);
    let dst = &mut [0_u8; 18];
    mem_ops.memcpy(dst, src, 18);
    assert_eq!(&src[..18], dst);
    let dst = &mut [0_u8; 18];
    mem_ops.memcpy(dst, &src[1..], 17);
    assert_eq!(&src[1..], &dst[..17]);
    let dst = &mut [0_u8; 18];
    mem_ops.memcpy(&mut dst[1..], &src[1..], 17);
    assert_eq!(&src[1..], &dst[1..]);

    // memmove
    unsafe {
        let buf = &mut [1_u8, 0];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[1] as *mut u8, 1);
        assert_eq!(buf[0], buf[1]);
        let buf = &mut [1_u8, 0];
        mem_ops.memmove(&mut buf[1] as *mut u8, &mut buf[0] as *mut u8, 1);
        assert_eq!(buf[0], buf[1]);
        let buf = &mut [1_u8, 1, 1, 0, 0, 0];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[3] as *mut u8, 3);
        assert_eq!(buf[..3], buf[3..]);
        let buf = &mut [1_u8, 1, 1, 0, 0, 0];
        mem_ops.memmove(&mut buf[3] as *mut u8, &mut buf[0] as *mut u8, 3);
        assert_eq!(buf[..3], buf[3..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[8] as *mut u8, 8);
        assert_eq!(buf[..8], buf[8..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        mem_ops.memmove(&mut buf[8] as *mut u8, &mut buf[0] as *mut u8, 8);
        assert_eq!(buf[..8], buf[8..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[9] as *mut u8, 9);
        assert_eq!(buf[..9], buf[9..]);
        let buf = &mut [0_u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        mem_ops.memmove(&mut buf[1] as *mut u8, &mut buf[0] as *mut u8, 9);
        assert_eq!(&mut [0_u8, 0, 1, 2, 3, 4, 5, 6, 7, 8], buf);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        mem_ops.memmove(&mut buf[9] as *mut u8, &mut buf[0] as *mut u8, 9);
        assert_eq!(buf[..9], buf[9..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[16] as *mut u8, 16);
        assert_eq!(buf[..16], buf[16..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[16] as *mut u8, &mut buf[0] as *mut u8, 16);
        assert_eq!(buf[..16], buf[16..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[18] as *mut u8, 18);
        assert_eq!(buf[..18], buf[18..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[18] as *mut u8, &mut buf[0] as *mut u8, 18);
        assert_eq!(buf[..18], buf[18..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[1] as *mut u8, &mut buf[18] as *mut u8, 17);
        assert_eq!(buf[1..17], buf[18..34]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        mem_ops.memmove(&mut buf[19] as *mut u8, &mut buf[1] as *mut u8, 17);
        assert_eq!(buf[..17], buf[19..]);
        let buf = &mut [0_u8, 0, 0, 1, 1, 1, 1, 1, 0];
        mem_ops.memmove(&mut buf[0] as *mut u8, &mut buf[3] as *mut u8, 5);
        assert_eq!(buf, &mut [1, 1, 1, 1, 1, 1, 1, 1, 0]);
    }

    // memset
    let exp = &[1_u8; 18];
    let buf = &mut [0_u8; 18];
    mem_ops.memset(&mut buf[0..], 1, 1);
    assert_eq!(exp[..1], buf[..1]);
    mem_ops.memset(&mut buf[0..], 1, 3);
    assert_eq!(exp[..3], buf[..3]);
    mem_ops.memset(&mut buf[0..], 1, 8);
    assert_eq!(exp[..8], buf[..8]);
    mem_ops.memset(&mut buf[0..], 1, 9);
    assert_eq!(exp[..9], buf[..9]);
    mem_ops.memset(&mut buf[0..], 1, 16);
    assert_eq!(exp[..16], buf[..16]);
    mem_ops.memset(&mut buf[0..], 1, 18);
    assert_eq!(exp[..18], buf[..18]);
    mem_ops.memset(&mut buf[1..], 1, 17);
    assert_eq!(exp[1..18], buf[1..18]);

    // memcmp
    assert_eq!(-1, mem_ops.memcmp(&[0_u8], &[1_u8], 1));
    assert_eq!(-1, mem_ops.memcmp(&[0_u8, 0, 0], &[0_u8, 0, 1], 3));
    assert_eq!(
        0,
        mem_ops.memcmp(
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0],
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0],
            9
        )
    );
    assert_eq!(
        -1,
        mem_ops.memcmp(
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0],
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 1],
            9
        )
    );
    assert_eq!(
        -1,
        mem_ops.memcmp(
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            10
        )
    );
    assert_eq!(0, mem_ops.memcmp(&[0_u8; 8], &[0_u8; 8], 8));
    assert_eq!(-1, mem_ops.memcmp(&[0_u8; 8], &[1_u8; 8], 8));
    assert_eq!(-1, mem_ops.memcmp(&[0_u8; 16], &[1_u8; 16], 16));
    assert_eq!(-1, mem_ops.memcmp(&[0_u8; 18], &[1_u8; 18], 18));
    let one = &[0_u8; 18];
    let two = &[1_u8; 18];
    assert_eq!(-1, mem_ops.memcmp(&one[1..], &two[0..], 17));
    assert_eq!(-1, mem_ops.memcmp(&one[1..], &two[1..], 17));
}
