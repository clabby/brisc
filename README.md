<h1>
  Brisc
  <a href="https://github.com/clabby/brisc/actions/workflows/rust_ci.yaml"><img src="https://github.com/clabby/brisc/actions/workflows/rust_ci.yaml/badge.svg?label=ci" alt="CI"></a>
  <img src="https://img.shields.io/badge/License-MIT-green.svg?label=license&labelColor=2a2f35" alt="License">
  <a href="https://codecov.io/github/clabby/brisc"><img src="https://codecov.io/github/clabby/brisc/graph/badge.svg?token=NLWBJYJJ4T" /></a>
</h1>

<img src="./assets/banner.png" alt="Brisc" align="right" width="250px" align="center">

Brisc is a collection of libraries for running single-threaded RISC-V programs targeting the unprivileged
`riscv{32/64}i{mac}` ISAs.

`brisc` is in active development and is not ready for production use.

## Crates

Each crate builds on the one before it:

- [`brisc-isa`](./crates/isa): instructions, with their decoding, encoding and assembly.
- [`brisc-jit`](./crates/jit): a [Cranelift](https://cranelift.dev) compiler for RISC-V regions. It is a library
  for `brisc-hw`, not something you call yourself.
- [`brisc-hw`](./crates/hw): the hart, an interpreter with its registers, the `Memory` trait that it reaches memory
  and devices through, and a sparse RAM. With the `jit` feature it also compiles hot loops.
- [`brisc-emu`](./crates/emu): a hart on a memory and a kernel of your choice, sync or async, and an ELF loader.

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md)
