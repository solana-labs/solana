//#![feature(test)]
//
//extern crate solana_core;
//extern crate test;
//
//use solana_core::chacha::chacha_cbc_encrypt_files;
//use std::fs::remove_file;
//use std::fs::File;
//use std::io::Write;
//use std::path::Path;
//use test::Bencher;
//
//#[bench]
//fn bench_chacha_encrypt(bench: &mut Bencher) {
//    let in_path = Path::new("bench_chacha_encrypt_file_input.txt");
//    let out_path = Path::new("bench_chacha_encrypt_file_output.txt.enc");
//    {
//        let mut in_file = File::create(in_path).unwrap();
//        for _ in 0..1024 {
//            in_file.write("123456foobar".as_bytes()).unwrap();
//        }
//    }
//    bench.iter(move || {
//        chacha_cbc_encrypt_files(in_path, out_path, "thetestkey".to_string()).unwrap();
//    });
//
//    remove_file(in_path).unwrap();
//    remove_file(out_path).unwrap();
//}
