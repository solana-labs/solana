use inkwell::context::Context;
use libsolenoid::evm::{Disassembly, Instruction};
use libsolenoid::compiler::Compiler;
use std::process::Command;
use uint::rustc_hex::FromHex;
use structopt::StructOpt;
use std::path::PathBuf;
use serde_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use log::{info, debug};

#[derive(Debug, StructOpt)]
#[structopt(name = "solenoid", about = "solenoid compiler toolchain")]
struct Opt {
    /// print opcodes then exit
    #[structopt(short, long)]
    print_opcodes: bool,

    /// debug
    #[structopt(short, long)]
    debug: bool,

    /// Input contract
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct Contract {
    abi: String,
    bin: String,
    #[serde(rename="bin-runtime")]
    bin_runtime: String,
}

impl Contract {
    pub fn parse(&self) -> (Vec<u8>, Vec<u8>, Vec<(usize, Instruction)>, Vec<(usize, Instruction)>) {
        let ctor_bytes: Vec<u8> = (self.bin).from_hex().expect("Invalid Hex String");
        let ctor_opcodes =  Disassembly::from_bytes(&ctor_bytes).unwrap().instructions;

        let rt_bytes: Vec<u8> = (self.bin_runtime).from_hex().expect("Invalid Hex String");
        let rt_opcodes =  Disassembly::from_bytes(&rt_bytes).unwrap().instructions;
        (ctor_bytes, rt_bytes, ctor_opcodes, rt_opcodes)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Contracts {
    contracts: HashMap<String, Contract>,
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();

    let cmd = Command::new("solc")
            .arg(opt.input)
            .arg("--combined-json")
            .arg("bin,bin-runtime,abi")
            .arg("--allow-paths=/")
            .output()
            .expect("solc command failed to start");
    let json = String::from_utf8_lossy(&cmd.stdout);

    let contracts: Contracts = serde_json::from_str(&json).unwrap();

    let context = Context::create();
    let module = context.create_module("contracts");
    for (name, contract) in &contracts.contracts {
        let name = name.split(":").last().unwrap();
        let builder = context.create_builder();
        let mut compiler = Compiler::new(&context, &module, opt.debug);
        let (ctor_bytes, rt_bytes, ctor_opcodes, rt_opcodes) = contract.parse();

        debug!("Constructor instrs: {:#?}", ctor_opcodes);
        debug!("Runtime instrs: {:#?}", rt_opcodes);

        if opt.print_opcodes {
            continue;
        }

        info!("Compiling {} constructor", name);
        compiler.compile(&builder, &ctor_opcodes, &ctor_bytes, name, false);

        info!("Compiling {} runtime", name);
        compiler.compile(&builder, &rt_opcodes, &rt_bytes, name, true);

        compiler.compile_abi(&builder, &contract.abi);
    }
        if opt.print_opcodes {
            return;
        }
    module.print_to_file("out.ll").unwrap();
}
