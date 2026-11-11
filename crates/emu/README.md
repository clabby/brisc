# `brisc-emu`

This crate contains a RISC-V emulator. An `Emulator` puts together a hart from `brisc-hw`, the bus it runs on, a
kernel that services its system calls, and a context that the kernel works on:

- The bus is any `brisc_hw::bus::Bus`: `SparseMemory`, or memory and devices of your own.
- The kernel is a `Kernel` or an `AsyncKernel`. `MinimalKernel` has the `exit` and `write` system
  calls, which is enough for programs that only compute and print.
- The context is state outside the kernel that every system call receives. The emulator hands
  it back with `Emulator::take_ctx`.

The hart is configured through `Emulator::cpu`, and the crate has an ELF loader to put programs on
the bus.

## Features

### RISC-V Extensions

With no features enabled, this crate serves a VM emulating the `rv32i` ISA. However, it can be extended with the
following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.

### JIT compiler

The `jit` feature adds a compiler for hot loops, which is off by default because it needs the
standard library and a 64-bit hart. Use `Emulator::with_jit` to configure it. See `brisc-hw` for what it does.

## Usage

```rust
use brisc_emu::{
    kernel::{Kernel, Syscall, SyscallError},
    Emulator,
};
use brisc_hw::{
    bus::Bus,
    cpu::Registers,
    memory::SparseMemory,
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

    fn syscall<B: Bus>(
        &mut self,
        sysno: XWord,
        bus: &mut B,
        registers: &mut Registers,
        output: &mut String,
    ) -> Result<Syscall, SyscallError<XWord, B::Error>> {
        match sysno {
            // exit
            0x5D => Ok(Syscall::Exit(registers.get(REG_A0))),
            // write
            0x40 => {
                let mut msg = vec![0; registers.get(REG_A2) as usize];
                bus.read(registers.get(REG_A1), &mut msg)?;
                output.push_str(&String::from_utf8_lossy(&msg));
                Ok(Syscall::Continue)
            }
            _ => Err(SyscallError::Kernel(sysno)),
        }
    }
}

let elf = const_hex::decode(HELLO_WORLD_ELF).unwrap();
let mut emu = Emulator::new(SparseMemory::new(), ExampleKernel, String::new());
emu.load_elf(&elf).unwrap();

let exit_code = emu.run().unwrap();
assert_eq!(exit_code, 0);
assert_eq!(emu.take_ctx(), "Hello, world!\n");
```

A kernel that needs to wait for something implements `AsyncKernel` instead and is driven by
`Emulator::run_async`. Both kinds of kernel are available in the same build.

`Emulator::run` returns an `EmulationError` when the guest executes something that is not an
instruction, executes an `ebreak`, when the bus fails an access, and when the kernel fails a
system call. After the first three the emulator is as it was before the instruction that failed, so
the caller can inspect it, repair the cause and run it again. After a failed system call, running again
makes the call again, and the kernel may have changed things before it failed.
