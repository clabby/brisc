<h1>
  Brisc
  <a href="https://github.com/clabby/brisc/actions/workflows/rust_ci.yaml"><img src="https://github.com/clabby/brisc/actions/workflows/rust_ci.yaml/badge.svg?label=ci" alt="CI"></a>
  <img src="https://img.shields.io/badge/License-MIT-green.svg?label=license&labelColor=2a2f35" alt="License">
  <a href="https://codecov.io/github/clabby/brisc"><img src="https://codecov.io/github/clabby/brisc/graph/badge.svg?token=NLWBJYJJ4T" /></a>
</h1>

<img src="./assets/banner.png" alt="Brisc" align="right" width="250px" align="center">

Brisc is a collection of libraries that assemble a RISC-V runtime. It is intended for executing single-threaded
programs targeting the unprivileged `riscv{32/64}i{mac}` ISAs.

### Development Status

`brisc` is currently in active development, and is not yet ready for use in production.

## Overview

Each crate builds on the one before it:

- [`brisc-isa`](./crates/isa) - The instruction set: architecture types, instructions, and their decoding, encoding and assembly.
- [`brisc-hw`](./crates/hw) - The hart: an interpreter with its register file, the bus interface it reaches memory and devices through, and a memory.
- [`brisc-emu`](./crates/emu) - The emulator: a hart on a bus of your choice with a kernel of your choice, sync or async, and an ELF loader.

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md)
