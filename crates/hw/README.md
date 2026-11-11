# `brisc-hw`

A RISC-V hart and the hardware it runs on:

- `cpu` is the processor: the register file, the program counter and the interpreter.
- `bus` is the `Bus` trait, through which the processor reaches memory and devices.
- `memory` is `SparseMemory`, a bus that is all memory.

The crate is `no_std` and needs `alloc`. It re-exports `brisc-isa`.

```rust
use brisc_hw::{
    bus::Bus,
    cpu::{Cpu, Stop},
    memory::SparseMemory,
    rv_code, REG_A0,
};

let mut memory = SparseMemory::new();
let code = rv_code! {
    addi a0, zero, 2;
    slli a0, a0, 4;
    ecall;
};
let Ok(()) = memory.write(0x1000, &code);

let mut cpu = Cpu::new(0x1000);
// Retire exactly one instruction, then run up to the system call.
assert_eq!(cpu.run(&mut memory, 1), Stop::Budget);
assert_eq!(cpu.registers.get(REG_A0), 2);
assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
assert_eq!(cpu.registers.get(REG_A0), 32);
assert_eq!((cpu.pc, cpu.instret), (0x1008, 3));
```

## The processor

`Cpu::run` executes instructions until it has retired the number it was asked for, and returns
earlier at an `ecall`, at an `ebreak`, at an instruction that cannot be executed, and when the
bus fails an access. It never calls a kernel. Whoever drives the hart decides what a system call
does, changes the registers and the program counter, and calls `run` again.

Each instruction is decoded once and kept in decoded form, per page. The bus tells the
processor about writes to pages it has decoded from, so code that changes is decoded again
without the guest having to execute `fence.i`.

Privilege levels, interrupts and address translation are not modelled. Control and status
registers read as zero and ignore writes, and writing to a read-only one is a fault. The
documentation of `Cpu` has the details.

## The bus

A bus needs two methods, `load` and `store`, and chooses the type of its errors. A bus that
cannot fail uses `Infallible`, and the processor's checks compile to nothing. Instruction
fetches, bulk reads and writes, and the reporting of writes to code have default
implementations that a bus can replace. The documentation of `Bus` describes the contract.

## Features

With no features enabled, this crate serves a VM emulating the `rv32i` ISA. However, it can be extended with the
following features:
* `64-bit` - Enable the 64-bit RISC-V architecture and accompanying instructions.
* `m` - Standard Extension for Integer Multiplication and Division.
* `a` - Standard Extension for Atomic Instructions
* `c` - Standard Extension for Compressed Instructions.
