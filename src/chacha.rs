use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::io::{BufReader, BufWriter};
use std::path::Path;

const CHACHA_IVEC_SIZE: usize = 64;

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

pub fn chacha_cbc_encrypt_files(in_path: &Path, out_path: &Path, key: String) -> io::Result<()> {
    let mut in_file = BufReader::new(File::open(in_path).expect("Can't open ledger data file"));
    let mut out_file =
        BufWriter::new(File::create(out_path).expect("Can't open ledger encrypted data file"));
    let mut buffer = [0; 4 * 1024];
    let mut encrypted_buffer = [0; 4 * 1024];
    let mut ivec = [0; CHACHA_IVEC_SIZE];

    while let Ok(size) = in_file.read(&mut buffer) {
        debug!("read {} bytes", size);
        if size == 0 {
            break;
        }
        chacha_cbc_encrypt(
            &buffer[..size],
            &mut encrypted_buffer[..size],
            key.as_bytes(),
            &mut ivec,
        );
        if let Err(res) = out_file.write(&encrypted_buffer[..size]) {
            println!("Error writing file! {:?}", res);
            return Err(res);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chacha::chacha_cbc_encrypt_files;
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
        assert!(chacha_cbc_encrypt_files(in_path, out_path, "thetestkey".to_string()).is_ok());
        let mut out_file = File::open(out_path).unwrap();
        let mut buf = vec![];
        let size = out_file.read_to_end(&mut buf).unwrap();
        assert_eq!(
            buf[..size],
            [106, 186, 59, 108, 165, 33, 118, 212, 70, 238, 205, 185]
        );
        remove_file(in_path).unwrap();
        remove_file(out_path).unwrap();
    }
}
