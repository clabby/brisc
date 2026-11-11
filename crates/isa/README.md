# `brisc-isa`

The RISC-V instruction set as data: the types of the architecture, the instructions, and their
encodings. The crate is `no_std`, allocates nothing, and knows nothing about how instructions
are executed.

- `XWord` and its relatives are the integer types of the architecture, and `REG_*` and
  `REGISTER_NAMES` name the integer registers.
- `Opcode` has a variant for every instruction, and `Instruction` is an opcode with its operands:
  `rd`, `rs1`, `rs2` and an immediate.
- `Instruction::decode` turns a word into an `Instruction` and `Instruction::encode` turns it
  back. Both are `const fn`. Decoding accepts exactly the instructions of the enabled instruction
  sets, and a 16-bit instruction of the C extension decodes to the instruction it abbreviates.
- `RType`, `IType` and the other formats give access to the raw fields of a word, and
  `CompressedInstruction` to the 16-bit instructions before they are expanded.
- `rv!` and `rv_code!` assemble instructions when the program is compiled, which is mostly
  useful for tests.

```rust
use brisc_isa::{rv, rv_code, Instruction, Opcode, REG_A0, REG_SP};

let instruction = Instruction::decode(0xff01_0513).unwrap();
assert_eq!(instruction.opcode, Opcode::Addi);
assert_eq!((instruction.rd as usize, instruction.rs1 as usize), (REG_A0, REG_SP));
assert_eq!(instruction.imm, -16);
assert_eq!(instruction.to_string(), "addi a0, sp, -16");

assert_eq!(instruction.encode(), Some(0xff01_0513));
assert_eq!(rv!(addi a0, sp, -16), 0xff01_0513);

// The machine code of a loop that counts a0 down to zero.
let code: [u8; 8] = rv_code! {
    addi a0, a0, -1;
    bne a0, zero, -4;
};
assert_eq!(code[..4], 0xfff5_0513u32.to_le_bytes());
```

## Features

With no features enabled, this crate serves the `rv32i` ISA. However, it can be extended with the following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.

The instructions of `Zicsr` and `Zifencei` and the privileged instructions `mret`, `sret`,
`wfi` and `sfence.vma` are always decoded.
