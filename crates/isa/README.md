# `brisc-isa`

A library defining RISC-V types as well as standard instruction types decoding functionality for them.

With no features enabled, this crate serves the `rv32i` ISA. However, it can be extended with the following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.
