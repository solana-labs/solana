use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub const CHACHA_BLOCK_SIZE: usize = 64;
pub const CHACHA_KEY_SIZE: usize = 32;

#[link(name = "cpu-crypt")]
extern "C" {
    fn chacha20_cbc_encrypt(
        input: *const u8,
        output: *mut u8,
        in_len: usize,
        key: *const u8,
        ivec: *mut u8,
    );
}

pub fn chacha_cbc_encrypt(input: &[u8], output: &mut [u8], key: &[u8], ivec: &mut [u8]) {
    unsafe {
        chacha20_cbc_encrypt(
            input.as_ptr(),
            output.as_mut_ptr(),
            input.len(),
            key.as_ptr(),
            ivec.as_mut_ptr(),
        );
    }
}

pub fn chacha_cbc_encrypt_file(
    in_path: &Path,
    out_path: &Path,
    ivec: &mut [u8; CHACHA_BLOCK_SIZE],
) -> io::Result<()> {
    let mut in_file = BufReader::new(File::open(in_path).expect("Can't open ledger data file"));
    let mut out_file =
        BufWriter::new(File::create(out_path).expect("Can't open ledger encrypted data file"));
    let mut buffer = [0; 4 * 1024];
    let mut encrypted_buffer = [0; 4 * 1024];
    let key = [0; CHACHA_KEY_SIZE];

    while let Ok(size) = in_file.read(&mut buffer) {
        debug!("read {} bytes", size);
        if size == 0 {
            break;
        }
        chacha_cbc_encrypt(&buffer[..size], &mut encrypted_buffer[..size], &key, ivec);
        if let Err(res) = out_file.write(&encrypted_buffer[..size]) {
            println!("Error writing file! {:?}", res);
            return Err(res);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chacha::chacha_cbc_encrypt_file;
    use std::fs::remove_file;
    use std::fs::File;
    use std::io::Read;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn test_encrypt_file() {
        let in_path = Path::new("test_chacha_encrypt_file_input.txt");
        let out_path = Path::new("test_chacha_encrypt_file_output.txt.enc");
        {
            let mut in_file = File::create(in_path).unwrap();
            in_file.write("123456foobar".as_bytes()).unwrap();
        }
        let mut key = hex!(
            "abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234
                            abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234"
        );
        assert!(chacha_cbc_encrypt_file(in_path, out_path, &mut key).is_ok());
        let mut out_file = File::open(out_path).unwrap();
        let mut buf = vec![];
        let size = out_file.read_to_end(&mut buf).unwrap();
        assert_eq!(
            buf[..size],
            [66, 54, 56, 212, 142, 110, 105, 158, 116, 82, 120, 53]
        );
        remove_file(in_path).unwrap();
        remove_file(out_path).unwrap();
    }
}
