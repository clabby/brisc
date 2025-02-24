<h1>
  Brisc
  <a href="https://github.com/anton-rs/kona/actions/workflows/rust_ci.yaml"><img src="https://github.com/anton-rs/kona/actions/workflows/rust_ci.yaml/badge.svg?label=ci" alt="CI"></a>
  <img src="https://img.shields.io/badge/License-MIT-green.svg?label=license&labelColor=2a2f35" alt="License">
  <a href="https://codecov.io/github/clabby/brisc"><img src="https://codecov.io/github/clabby/brisc/graph/badge.svg?token=NLWBJYJJ4T" /></a>
</h1>

<img src="./assets/banner.png" alt="Brisc" align="right" width="250px" align="center">

Brisc is a collection of libraries that assemble a RISC-V runtime. It is intended for executing single-threaded
programs targeting the unprivileged `riscv{32/64}i{mc}` ISAs.

### Development Status

`brisc` is currently in active development, and is not yet ready for use in production.

## Overview

- [`brisc-isa`](./crates/isa) - Types for supported instructions and decoding utilities.
- [`brisc-hw`](./crates/hw) - RISC-V Processor implementation (Pipeline stages, memory bus interface, register file, etc.)
- [`brisc-emu`](./crates/emu) - Single-Threaded RISC-V hart emulator with a 5-stage pipeline.

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md)
