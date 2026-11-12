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

## Benchmarks

Wall-clock time to run each 64-bit guest in `rv-tests/bin` to completion, on an Apple M4 Max with Rust 1.99. Each
figure is the best of nine runs. The JIT column includes the time the program spends before its loops are compiled, and
the compiler runs on a background thread. The geometric mean speedup is 2.4x.

| Program | Instructions | Interpreter | JIT | Speedup |
| --- | ---: | ---: | ---: | ---: |
| `bfvm` | 6.6 M | 8.0 ms (823 MIPS) | 6.1 ms (1081 MIPS) | 1.3x |
| `crc-rle` | 7.9 M | 7.1 ms (1111 MIPS) | 4.5 ms (1753 MIPS) | 1.6x |
| `fib` | 29.6 M | 26.1 ms (1135 MIPS) | 6.6 ms (4484 MIPS) | 3.9x |
| `hashmap` | 13.6 M | 14.2 ms (963 MIPS) | 5.9 ms (2311 MIPS) | 2.4x |
| `mandelbrot` | 6.6 M | 3.7 ms (1806 MIPS) | 1.0 ms (6551 MIPS) | 3.6x |
| `matmul` | 10.3 M | 7.3 ms (1397 MIPS) | 2.5 ms (4116 MIPS) | 2.9x |
| `quicksort-heap` | 38.8 M | 32.8 ms (1185 MIPS) | 12.3 ms (3169 MIPS) | 2.7x |
| `sha256` | 10.7 M | 7.2 ms (1479 MIPS) | 5.4 ms (1981 MIPS) | 1.3x |
| `sieve` | 19.3 M | 13.4 ms (1439 MIPS) | 2.9 ms (6727 MIPS) | 4.7x |
| `tree` | 19.2 M | 24.6 ms (780 MIPS) | 14.5 ms (1322 MIPS) | 1.7x |

Programs with tight arithmetic loops gain the most. Run the Criterion benchmarks with
`cargo bench -p brisc-emu --features test-utils,jit`.

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md)
