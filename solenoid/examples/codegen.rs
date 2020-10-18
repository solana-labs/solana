use inkwell::context::Context;
use libsolenoid::evm::{self, Instruction};
use libsolenoid::compiler::Compiler;

fn main() {
    let context = Context::create();
    let module = context.create_module("contract");
    let builder = context.create_builder();

    let instrs = vec![
        // Instruction::Push(vec![0x80]),
        // Instruction::Push(vec![0x40]),
        // Instruction::MStore,
        // Instruction::CallValue,
        // Instruction::Dup(1),
        // Instruction::IsZero,
        // Instruction::Push(vec![0x00, 0x10]),
        // Instruction::JumpIf,
        // Instruction::Push(vec![0]),
        // Instruction::Dup(1),
        // Instruction::Revert,
        // Instruction::JumpDest,
        // Instruction::Pop,

        // Instruction::Push(vec![0x2]),
        // Instruction::Push(vec![0x3]),
        // Instruction::Exp,

        // Instruction::Push(vec![0x74, 0x65, 0x73, 0x74]),
        // Instruction::Push(vec![0]),
        // Instruction::MStore,
        // Instruction::Push(vec![4]),
        // Instruction::Push(vec![0]),
        // Instruction::Sha3,

        // Instruction::Push(vec![0x41]),
        // Instruction::Push(vec![1]),
        // Instruction::SStore,
        // Instruction::Push(vec![1]),
        // Instruction::SLoad,

        // Instruction::Push(vec![228]),
        // Instruction::Push(vec![1]),
        // Instruction::SignExtend,

        Instruction::Push(vec![10]),
        Instruction::Push(vec![10]),
        Instruction::Div,
    ];
    let bytes = evm::assemble_instructions(instrs);
    let instrs = evm::Disassembly::from_bytes(&bytes).unwrap().instructions;

    let mut compiler = Compiler::new(&context, &module, false);
    compiler.compile(&builder, &instrs, &bytes, "test", false);
    compiler.dbg();
    module.print_to_file("out.ll").unwrap();
}
