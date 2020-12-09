//! @brief Test builtin mem functions

#![cfg(target_arch = "bpf")]
#![feature(compiler_builtins_lib)]

extern crate compiler_builtins;
use solana_program::{custom_panic_default, entrypoint::SUCCESS, info};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    unsafe {
        // memcpy
        let src = &mut [1_u8; 18];
        let dst = &mut [0_u8; 1];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 1);
        assert_eq!(&src[..1], dst);
        let dst = &mut [0_u8; 3];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 3);
        assert_eq!(&src[..3], dst);
        let dst = &mut [0_u8; 8];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 8);
        assert_eq!(&src[..8], dst);
        let dst = &mut [0_u8; 9];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 9);
        assert_eq!(&src[..9], dst);
        let dst = &mut [0_u8; 16];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 16);
        assert_eq!(&src[..16], dst);
        let dst = &mut [0_u8; 18];
        compiler_builtins::mem::memcpy(&mut src[0] as *mut u8, &mut dst[0] as *mut u8, 18);
        assert_eq!(&src[..18], dst);
        let dst = &mut [0_u8; 18];
        compiler_builtins::mem::memcpy(&mut src[1] as *mut u8, &mut dst[0] as *mut u8, 17);
        assert_eq!(&src[1..], &dst[1..]);
        let dst = &mut [0_u8; 18];
        compiler_builtins::mem::memcpy(&mut src[1] as *mut u8, &mut dst[1] as *mut u8, 17);
        assert_eq!(&src[1..], &dst[..17]);

        // memmove
        let buf = &mut [1_u8, 0];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[1] as *mut u8, 1);
        assert_eq!(buf[0], buf[1]);
        let buf = &mut [1_u8, 0];
        compiler_builtins::mem::memmove(&mut buf[1] as *mut u8, &mut buf[0] as *mut u8, 1);
        assert_eq!(buf[0], buf[1]);
        let buf = &mut [1_u8, 1, 1, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[3] as *mut u8, 3);
        assert_eq!(buf[..3], buf[3..]);
        let buf = &mut [1_u8, 1, 1, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[3] as *mut u8, &mut buf[0] as *mut u8, 3);
        assert_eq!(buf[..3], buf[3..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[8] as *mut u8, 8);
        assert_eq!(buf[..8], buf[8..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[8] as *mut u8, &mut buf[0] as *mut u8, 8);
        assert_eq!(buf[..8], buf[8..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[9] as *mut u8, 9);
        assert_eq!(buf[..9], buf[9..]);
        let buf = &mut [1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        compiler_builtins::mem::memmove(&mut buf[9] as *mut u8, &mut buf[0] as *mut u8, 9);
        assert_eq!(buf[..9], buf[9..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[16] as *mut u8, 16);
        assert_eq!(buf[..16], buf[16..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[16] as *mut u8, &mut buf[0] as *mut u8, 16);
        assert_eq!(buf[..16], buf[16..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[0] as *mut u8, &mut buf[18] as *mut u8, 18);
        assert_eq!(buf[..18], buf[18..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[18] as *mut u8, &mut buf[0] as *mut u8, 18);
        assert_eq!(buf[..18], buf[18..]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[1] as *mut u8, &mut buf[18] as *mut u8, 17);
        assert_eq!(buf[1..17], buf[18..34]);
        let buf = &mut [
            1_u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        compiler_builtins::mem::memmove(&mut buf[19] as *mut u8, &mut buf[1] as *mut u8, 17);
        assert_eq!(buf[..17], buf[19..]);

        // memset
        let exp = &[1_u8; 18];
        let buf = &mut [0_u8; 18];
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 1);
        assert_eq!(exp[..1], buf[..1]);
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 3);
        assert_eq!(exp[..3], buf[..3]);
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 8);
        assert_eq!(exp[..8], buf[..8]);
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 9);
        assert_eq!(exp[..9], buf[..9]);
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 16);
        assert_eq!(exp[..16], buf[..16]);
        compiler_builtins::mem::memset(&mut buf[0] as *mut u8, 1, 18);
        assert_eq!(exp[..18], buf[..18]);
        compiler_builtins::mem::memset(&mut buf[1] as *mut u8, 1, 17);
        assert_eq!(exp[1..18], buf[1..18]);

        // memcmp
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&[0_u8] as *const u8, &[1_u8] as *const u8, 1)
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(
                &[0_u8, 0, 0] as *const u8,
                &[0_u8, 0, 1] as *const u8,
                3
            )
        );
        assert_eq!(
            0,
            compiler_builtins::mem::memcmp(
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0] as *const u8,
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0] as *const u8,
                9
            )
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0] as *const u8,
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 1] as *const u8,
                9
            )
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0, 0] as *const u8,
                &[0_u8, 0, 0, 0, 0, 0, 0, 0, 0, 1] as *const u8,
                10
            )
        );
        assert_eq!(
            0,
            compiler_builtins::mem::memcmp(&[0_u8; 8] as *const u8, &[0_u8; 8] as *const u8, 8)
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&[0_u8; 8] as *const u8, &[1_u8; 8] as *const u8, 8)
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&[0_u8; 16] as *const u8, &[1_u8; 16] as *const u8, 16)
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&[0_u8; 18] as *const u8, &[1_u8; 18] as *const u8, 18)
        );
        let one = &[0_u8; 18];
        let two = &[1_u8; 18];
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&one[1] as *const u8, &two[0] as *const u8, 17)
        );
        assert_eq!(
            -1,
            compiler_builtins::mem::memcmp(&one[1] as *const u8, &two[1] as *const u8, 17)
        );
    }

    SUCCESS
}

custom_panic_default!();
