use rbpf::ebpf;

// This “verifier” performs simple checks when the eBPF program is loaded into the VM (before it is
// interpreted or JIT-compiled).

fn verify_prog_len(prog: &[u8]) {
    if prog.len() % ebpf::INSN_SIZE != 0 {
        panic!(
            "[Verifier] Error: eBPF program length must be a multiple of {:?} octets",
            ebpf::INSN_SIZE
        );
    }
    if prog.len() > ebpf::PROG_MAX_SIZE {
        panic!(
            "[Verifier] Error: eBPF program length limited to {:?}, here {:?}",
            ebpf::PROG_MAX_INSNS,
            prog.len() / ebpf::INSN_SIZE
        );
    }
    if prog.is_empty() {
        panic!("[Verifier] Error: program is empty");
    }
    // TODO BPF program may deterministically exit even if the last
    // instruction in the block is not an exit (might be earlier and jumped to)
    // TODO need to validate more intelligently
    // let last_insn = ebpf::get_insn(prog, (prog.len() / ebpf::INSN_SIZE) - 1);
    // if last_insn.opc != ebpf::EXIT {
    //     panic!("[Verifier] Error: program does not end with “EXIT” instruction");
    // }
}

fn verify_imm_nonzero(insn: &ebpf::Insn, insn_ptr: usize) {
    if insn.imm == 0 {
        panic!("[Verifier] Error: division by 0 (insn #{:?})", insn_ptr);
    }
}

fn verify_imm_endian(insn: &ebpf::Insn, insn_ptr: usize) {
    match insn.imm {
        16 | 32 | 64 => return,
        _ => panic!(
            "[Verifier] Error: unsupported argument for LE/BE (insn #{:?})",
            insn_ptr
        ),
    }
}

fn verify_load_dw(prog: &[u8], insn_ptr: usize) {
    // We know we can reach next insn since we enforce an EXIT insn at the end of program, while
    // this function should be called only for LD_DW insn, that cannot be last in program.
    let next_insn = ebpf::get_insn(prog, insn_ptr + 1);
    if next_insn.opc != 0 {
        panic!(
            "[Verifier] Error: incomplete LD_DW instruction (insn #{:?})",
            insn_ptr
        );
    }
}

fn verify_jmp_offset(prog: &[u8], insn_ptr: usize) {
    let insn = ebpf::get_insn(prog, insn_ptr);
    if insn.off == -1 {
        panic!("[Verifier] Error: infinite loop (insn #{:?})", insn_ptr);
    }

    let dst_insn_ptr = insn_ptr as isize + 1 + insn.off as isize;
    if dst_insn_ptr < 0 || dst_insn_ptr as usize >= (prog.len() / ebpf::INSN_SIZE) {
        panic!(
            "[Verifier] Error: jump out of code to #{:?} (insn #{:?})",
            dst_insn_ptr, insn_ptr
        );
    }

    let dst_insn = ebpf::get_insn(prog, dst_insn_ptr as usize);
    if dst_insn.opc == 0 {
        panic!(
            "[Verifier] Error: jump to middle of LD_DW at #{:?} (insn #{:?})",
            dst_insn_ptr, insn_ptr
        );
    }
}

fn verify_registers(insn: &ebpf::Insn, store: bool, insn_ptr: usize) {
    if insn.src > 10 {
        panic!(
            "[Verifier] Error: invalid source register (insn #{:?})",
            insn_ptr
        );
    }

    match (insn.dst, store) {
        (0...9, _) | (10, true) => {}
        (10, false) => panic!(
            "[Verifier] Error: cannot write into register r10 (insn #{:?})",
            insn_ptr
        ),
        (_, _) => panic!(
            "[Verifier] Error: invalid destination register (insn #{:?})",
            insn_ptr
        ),
    }
}

pub fn verifier(prog: &[u8]) -> bool {
    verify_prog_len(prog);

    let mut insn_ptr: usize = 0;
    while insn_ptr * ebpf::INSN_SIZE < prog.len() {
        let insn = ebpf::get_insn(prog, insn_ptr);
        let mut store = false;

        match insn.opc {
            // BPF_LD class
            ebpf::LD_ABS_B => {}
            ebpf::LD_ABS_H => {}
            ebpf::LD_ABS_W => {}
            ebpf::LD_ABS_DW => {}
            ebpf::LD_IND_B => {}
            ebpf::LD_IND_H => {}
            ebpf::LD_IND_W => {}
            ebpf::LD_IND_DW => {}

            ebpf::LD_DW_IMM => {
                store = true;
                verify_load_dw(prog, insn_ptr);
                insn_ptr += 1;
            }

            // BPF_LDX class
            ebpf::LD_B_REG => {}
            ebpf::LD_H_REG => {}
            ebpf::LD_W_REG => {}
            ebpf::LD_DW_REG => {}

            // BPF_ST class
            ebpf::ST_B_IMM => store = true,
            ebpf::ST_H_IMM => store = true,
            ebpf::ST_W_IMM => store = true,
            ebpf::ST_DW_IMM => store = true,

            // BPF_STX class
            ebpf::ST_B_REG => store = true,
            ebpf::ST_H_REG => store = true,
            ebpf::ST_W_REG => store = true,
            ebpf::ST_DW_REG => store = true,
            ebpf::ST_W_XADD => {
                unimplemented!();
            }
            ebpf::ST_DW_XADD => {
                unimplemented!();
            }

            // BPF_ALU class
            ebpf::ADD32_IMM => {}
            ebpf::ADD32_REG => {}
            ebpf::SUB32_IMM => {}
            ebpf::SUB32_REG => {}
            ebpf::MUL32_IMM => {}
            ebpf::MUL32_REG => {}
            ebpf::DIV32_IMM => {
                verify_imm_nonzero(&insn, insn_ptr);
            }
            ebpf::DIV32_REG => {}
            ebpf::OR32_IMM => {}
            ebpf::OR32_REG => {}
            ebpf::AND32_IMM => {}
            ebpf::AND32_REG => {}
            ebpf::LSH32_IMM => {}
            ebpf::LSH32_REG => {}
            ebpf::RSH32_IMM => {}
            ebpf::RSH32_REG => {}
            ebpf::NEG32 => {}
            ebpf::MOD32_IMM => {
                verify_imm_nonzero(&insn, insn_ptr);
            }
            ebpf::MOD32_REG => {}
            ebpf::XOR32_IMM => {}
            ebpf::XOR32_REG => {}
            ebpf::MOV32_IMM => {}
            ebpf::MOV32_REG => {}
            ebpf::ARSH32_IMM => {}
            ebpf::ARSH32_REG => {}
            ebpf::LE => {
                verify_imm_endian(&insn, insn_ptr);
            }
            ebpf::BE => {
                verify_imm_endian(&insn, insn_ptr);
            }

            // BPF_ALU64 class
            ebpf::ADD64_IMM => {}
            ebpf::ADD64_REG => {}
            ebpf::SUB64_IMM => {}
            ebpf::SUB64_REG => {}
            ebpf::MUL64_IMM => {
                verify_imm_nonzero(&insn, insn_ptr);
            }
            ebpf::MUL64_REG => {}
            ebpf::DIV64_IMM => {
                verify_imm_nonzero(&insn, insn_ptr);
            }
            ebpf::DIV64_REG => {}
            ebpf::OR64_IMM => {}
            ebpf::OR64_REG => {}
            ebpf::AND64_IMM => {}
            ebpf::AND64_REG => {}
            ebpf::LSH64_IMM => {}
            ebpf::LSH64_REG => {}
            ebpf::RSH64_IMM => {}
            ebpf::RSH64_REG => {}
            ebpf::NEG64 => {}
            ebpf::MOD64_IMM => {}
            ebpf::MOD64_REG => {}
            ebpf::XOR64_IMM => {}
            ebpf::XOR64_REG => {}
            ebpf::MOV64_IMM => {}
            ebpf::MOV64_REG => {}
            ebpf::ARSH64_IMM => {}
            ebpf::ARSH64_REG => {}

            // BPF_JMP class
            ebpf::JA => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JEQ_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JEQ_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JGT_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JGT_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JGE_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JGE_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JLT_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JLT_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JLE_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JLE_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSET_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSET_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JNE_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JNE_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSGT_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSGT_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSGE_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSGE_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSLT_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSLT_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSLE_IMM => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::JSLE_REG => {
                verify_jmp_offset(prog, insn_ptr);
            }
            ebpf::CALL => {}
            ebpf::TAIL_CALL => unimplemented!(),
            ebpf::EXIT => {}

            _ => {
                panic!(
                    "[Verifier] Error: unknown eBPF opcode {:#2x} (insn #{:?})",
                    insn.opc, insn_ptr
                );
            }
        }

        verify_registers(&insn, store, insn_ptr);

        insn_ptr += 1;
    }

    // insn_ptr should now be equal to number of instructions.
    if insn_ptr != prog.len() / ebpf::INSN_SIZE {
        panic!("[Verifier] Error: jumped out of code to #{:?}", insn_ptr);
    }

    true
}
