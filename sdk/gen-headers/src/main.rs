#![allow(clippy::integer_arithmetic)]

use {
    log::info,
    regex::{Captures, Regex},
    std::{
        ffi::OsStr,
        fs,
        io::{prelude::*, BufWriter},
        path::PathBuf,
        str,
    },
};

/**
 * 1. process every inc file in syscalls header file
 *
 * 2. in every such file replace the syscall declaration by a new
 * declaration with a new extended name, and a static function
 * definition that computes a hash of the original name and uses the
 * hash to initialize a function pointer, the function pointer then is
 * used the call the syscall function.
 */
fn main() {
    let syscalls_inc_path = PathBuf::from("sdk/sbf/c/inc/sol/inc");

    if syscalls_inc_path.is_dir() {
        for entry in fs::read_dir(syscalls_inc_path).expect("Can't open headers dir") {
            let entry = entry.expect("Can't open header file");
            let path = entry.path();
            if !path.is_dir() {
                let extension = path.extension();
                if extension == Some(OsStr::new("inc")) {
                    transform(&path);
                }
            }
        }
    }
}

/**
 * Transform input inc file to a valid C header file replacing
 * declaration templates with valid C code.
 */
fn transform(inc: &PathBuf) {
    let inc_path = PathBuf::from(inc);
    let filename = match inc_path.file_name() {
        Some(f) => f,
        None => return,
    };
    let parent = match inc_path.parent() {
        Some(f) => f,
        None => return,
    };
    let parent = match parent.parent() {
        Some(f) => f,
        None => return,
    };
    let mut header_path = PathBuf::from(parent);
    let mut filename = PathBuf::from(filename);
    filename.set_extension("h");
    header_path.push(filename);
    info!(
        "Transforming file {} -> {}",
        inc.display(),
        header_path.display()
    );
    let mut input = match fs::File::open(inc) {
        Ok(x) => x,
        Err(err) => panic!("Failed to open {}: {}", inc.display(), err),
    };
    let mut input_content = vec![];
    input.read_to_end(&mut input_content).unwrap();
    let input_content = str::from_utf8(&input_content).unwrap();
    let output = match fs::File::create(&header_path) {
        Ok(x) => x,
        Err(err) => panic!("Failed to create {}: {}", header_path.display(), err),
    };
    let mut output_writer = BufWriter::new(output);
    let decl_re =
        Regex::new(r"@SYSCALL ([0-9A-Za-z_*]+)[[:space:]]+(sol_[0-9A-Za-z_]+)\(([^);]*)\);")
            .unwrap();
    let comm_re = Regex::new(r",").unwrap();
    let output_content = decl_re.replace_all(input_content, |caps: &Captures| {
        let ty = &caps[1].to_string();
        let func = &caps[2].to_string();
        let args = &caps[3].to_string();
        let warn = format!("/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE {} AND RUN `cargo run --bin gen-headers` */", inc.display());
        let ifndef = format!("#ifndef SOL_SBFV2\n{ty} {func}({args});");
        let hash = sys_hash(func);
        let typedef_statement = format!("typedef {ty}(*{func}_pointer_type)({args});");
        let mut arg = 0;
        let mut arg_list = "".to_string();
        if !args.is_empty() {
            arg_list = comm_re
                .replace_all(args, |_caps: &Captures| {
                    arg += 1;
                    format!(" arg{arg},")
                })
                .to_string();
            arg += 1;
            arg_list = format!("{arg_list} arg{arg}");
        }
        let function_signature = format!("static {ty} {func}({arg_list})");
        let pointer_assignment = format!(
            "{func}_pointer_type {func}_pointer = ({func}_pointer_type) {hash};"
        );
        if !args.is_empty() {
            arg_list = "arg1".to_string();
            for a in 2..arg + 1 {
                arg_list = format!("{arg_list}, arg{a}");
            }
        }
        let return_statement = if ty == "void" {
            format!("{func}_pointer({arg_list});")
        } else {
            format!("return {func}_pointer({arg_list});")
        };
        format!(
            "{warn}\n{ifndef}\n#else\n{typedef_statement}\n{function_signature} {{\n  {pointer_assignment}\n  {return_statement}\n}}\n#endif",
        )
    });
    write!(output_writer, "{output_content}").unwrap();
}

const fn sys_hash(name: &str) -> usize {
    murmur3_32(name.as_bytes(), 0) as usize
}

#[inline(always)]
const fn murmur3_32(buf: &[u8], seed: u32) -> u32 {
    let mut hash = seed;
    let mut i = 0;
    while i < buf.len() / 4 {
        let buf = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
        hash ^= pre_mix(buf);
        hash = hash.rotate_left(13);
        hash = hash.wrapping_mul(5).wrapping_add(0xe6546b64);

        i += 1;
    }
    match buf.len() % 4 {
        0 => {}
        1 => {
            hash ^= pre_mix([buf[i * 4], 0, 0, 0]);
        }
        2 => {
            hash ^= pre_mix([buf[i * 4], buf[i * 4 + 1], 0, 0]);
        }
        3 => {
            hash ^= pre_mix([buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], 0]);
        }
        _ => { /* unreachable!() */ }
    }

    hash ^= buf.len() as u32;
    hash ^= hash.wrapping_shr(16);
    hash = hash.wrapping_mul(0x85ebca6b);
    hash ^= hash.wrapping_shr(13);
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash ^= hash.wrapping_shr(16);

    hash
}

const fn pre_mix(buf: [u8; 4]) -> u32 {
    u32::from_le_bytes(buf)
        .wrapping_mul(0xcc9e2d51)
        .rotate_left(15)
        .wrapping_mul(0x1b873593)
}
