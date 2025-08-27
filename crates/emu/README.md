# `brisc`

This crate contains a simple RISC-V emulator.

## Features

### RISC-V Extensions

With no features enabled, this crate serves a VM emulating the `rv32i` ISA. However, it can be extended with the
following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.

## Usage

```rust
use brisc_emu::{cfg::EmuConfig, st::StEmu};
use brisc_hw::{
    errors::PipelineResult,
    kernel::Kernel,
    memory::{Memory, SimpleMemory},
    pipeline::PipelineRegister,
    XWord, REG_A0, REG_A1, REG_A2,
};

/// .section .data
/// hello_msg:
///     .ascii "Hello, world!\n"
/// hello_len = . - hello_msg
///
/// .section .text
/// .global _start
///
/// _start:
///     # write system call
///     # a0 = file descriptor (1 for stdout)
///     # a1 = buffer address
///     # a2 = number of bytes to write
///     # a7 = system call number (64 for write)
///
///     li a0, 1                    # stdout file descriptor
///     la a1, hello_msg            # load address of message
///     li a2, hello_len            # length of message
///     li a7, 64                   # write system call number
///     ecall                       # invoke system call
///
///     # exit system call
///     # a0 = exit status
///     # a7 = system call number (93 for exit)
///
///     li a0, 0                    # exit status 0 (success)
///     li a7, 93                   # exit system call number
///     ecall                       # invoke system call
const HELLO_WORLD_ELF: &str = "7f454c460201010000000000000000000200f30001000000e8000100000000004000000000000000380100000000000004000000400038000300400004000300030000700400000000000000000000000000000000000000000000000000000000000000000000000000000000000000080000000000000001000000050000000000000000000000000001000000000000000100000000000c010000000000000c01000000000000001000000000000001000000060000000c010000000000000c110100000000000c110100000000000e000000000000000e0000000000000000100000000000001305100097150000938505021306e0009308000473000000130500009308d0057300000048656c6c6f2c20776f726c64210a002e7368737472746162002e74657874002e646174610000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b000000010000000600000000000000e800010000000000e8000000000000002400000000000000000000000000000004000000000000000000000000000000110000000100000003000000000000000c110100000000000c010000000000000e000000000000000000000000000000010000000000000000000000000000000100000003000000000000000000000000000000000000001a010000000000001700000000000000000000000000000001000000000000000000000000000000";

#[derive(Default)]
struct ExampleKernel;

impl Kernel for ExampleKernel {
    fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        p_reg: &mut PipelineRegister,
    ) -> PipelineResult<XWord> {
        match sysno {
            0x5D => {
                let exit_code = p_reg.registers[REG_A0 as usize];
                p_reg.exit_code = exit_code;
                p_reg.exit = true;
            }
            0x40 => {
                let fd = p_reg.registers[REG_A0 as usize];
                let ptr = p_reg.registers[REG_A1 as usize];
                let len = p_reg.registers[REG_A2 as usize];

                let raw_msg = mem.read_memory_range(ptr, len).unwrap();
                let msg = String::from_utf8_lossy(&raw_msg);

                match fd {
                    1 => println!("{msg}"),
                    2 => eprintln!("{msg}"),
                    _ => panic!("Unknown file descriptor: {msg}"),
                }
            }
            _ => panic!("Unknown system call: {sysno}"),
        }

        Ok(0)
    }
}

#[derive(Default)]
struct ExampleEmuConfig;

impl EmuConfig for ExampleEmuConfig {
    type Memory = SimpleMemory;
    type Kernel = ExampleKernel;
}

let elf = const_hex::decode(HELLO_WORLD_ELF).unwrap();
let mut emu = StEmu::<ExampleEmuConfig>::builder()
    .with_kernel(ExampleKernel)
    .with_elf(&elf)
    .unwrap()
    .build();

emu.run().unwrap();
```
