//! A Brainfuck interpreter running a nested-loop program: an opcode dispatch loop with
//! unpredictable branches, like a bytecode virtual machine.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;

const PROGRAM: &[u8] = b"++++++++++++++++[>++++++++++++++++[>++++++++++++++++[>++++++++++++++++[>+<-]<-]<-]<-]>>>>.";

fn kernel() -> u64 {
    // Resolve the jump targets first.
    let mut jump = [0usize; PROGRAM.len()];
    let mut stack = [0usize; 32];
    let mut depth = 0;
    for (pc, op) in PROGRAM.iter().enumerate() {
        match op {
            b'[' => {
                stack[depth] = pc;
                depth += 1;
            }
            b']' => {
                depth -= 1;
                jump[pc] = stack[depth];
                jump[stack[depth]] = pc;
            }
            _ => {}
        }
    }

    let mut tape = [0u8; 64];
    let (mut pc, mut ptr) = (0usize, 0usize);
    let mut steps = 0u64;
    while pc < PROGRAM.len() {
        match PROGRAM[pc] {
            b'>' => ptr += 1,
            b'<' => ptr -= 1,
            b'+' => tape[ptr] = tape[ptr].wrapping_add(1),
            b'-' => tape[ptr] = tape[ptr].wrapping_sub(1),
            b'[' => {
                if tape[ptr] == 0 {
                    pc = jump[pc];
                }
            }
            b']' => {
                if tape[ptr] != 0 {
                    pc = jump[pc];
                }
            }
            _ => {}
        }
        pc += 1;
        steps += 1;
    }
    steps << 16 ^ tape.iter().fold(0u64, |acc, v| acc * 31 + *v as u64)
}

entry!(kernel, 27487764480);

