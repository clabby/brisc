# `brisc-hw`

This crate contains the hardware of a RISC-V hart: the processor (`cpu`) and the guest memory it
executes from (`memory`). For convenience and portability, the crate is `no_std` w/ `alloc`
requirements with no default features enabled.

The processor decodes each instruction once and caches the decoded form per page of guest memory.
Writes to a page drop its decoded instructions, so self-modifying code works without `fence.i`.
`Cpu::run` retires an exact number of instructions and returns at every `ecall`; it never calls a
kernel itself.

## Features

### RISC-V Extensions

With no features enabled, this crate serves a VM emulating the `rv32i` ISA. However, it can be extended with the
following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.
