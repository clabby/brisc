# `brisc-emu`

This crate contains a single-threaded RISC-V emulator: a hart from `brisc-hw`, its memory, an ELF
loader, and the loop that hands system calls to a kernel.

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
use brisc_emu::{
    cfg::EmuConfig,
    kernel::{Kernel, Syscall},
    st::StEmu,
};
use brisc_hw::{
    cpu::Registers,
    memory::{Memory, SparseMemory},
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

struct ExampleKernel;

/// The kernel appends everything the program writes to the `String` it is handed as its state.
impl Kernel<String> for ExampleKernel {
    /// The number of the unsupported system call.
    type Error = XWord;

    fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        registers: &mut Registers,
        output: &mut String,
    ) -> Result<Syscall, Self::Error> {
        match sysno {
            // exit
            0x5D => Ok(Syscall::Exit(registers.get(REG_A0))),
            // write
            0x40 => {
                let mut msg = vec![0; registers.get(REG_A2) as usize];
                mem.read(registers.get(REG_A1), &mut msg);
                output.push_str(&String::from_utf8_lossy(&msg));
                Ok(Syscall::Continue)
            }
            _ => Err(sysno),
        }
    }
}

struct ExampleEmuConfig;

impl EmuConfig<'_> for ExampleEmuConfig {
    type Memory = SparseMemory;
    type Kernel = ExampleKernel;
    type Context = String;
}

let elf = const_hex::decode(HELLO_WORLD_ELF).unwrap();
let mut emu = StEmu::<ExampleEmuConfig>::builder()
    .with_kernel(ExampleKernel)
    .with_ctx(String::new())
    .with_elf(&elf)
    .unwrap()
    .build();

let exit_code = emu.run().unwrap();
assert_eq!(exit_code, 0);
assert_eq!(emu.take_ctx(), "Hello, world!\n");
```

A kernel that needs to wait for something implements `AsyncKernel` instead and is driven by
`StEmu::run_async`. Both kinds of kernel are available in the same build.
