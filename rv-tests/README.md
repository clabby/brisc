# `rv-tests`

This directory contains test vectors for the `brisc` emulator, namely:
- the [`riscv-tests`][rv-tests] submodule, containing test suites for each instruction supported by the emulator.
- sample `no_std` Rust programs, for benchmarking.

## `riscv-tests` Test Suites

For the sake of `brisc`, we only care about a few of the test suites that are available in the upstream spec tests:

- `rv32ui-p-*`: RISC-V 32-bit base ISA tests. User-level, virtual memory disabled, single-hart.
- `rv32um-p-*`: RISC-V `m` extension tests. User-level, virtual memory disabled, single-hart.
- `rv64ui-p-*`: RISC-V 64-bit base ISA tests. User-level, virtual memory disabled, single-hart.
- `rv64um-p-*`: RISC-V 64-bit `m` extension tests. User-level, virtual memory disabled, single-hart.

### Test Suite Semantics

- The `riscv-tests` submodule will contain all of the compiled test suite ELF files in the `isa` directory.
    - Accompanying the ELF binaries, a `.dump` file is available with human-readable assembly.
- Each test suite's entry point is constant at `0x80000000`
- If the test fails:
    - Register `a0` is set to `(test_number << 1) | 1`
    - Register `a7` is set to `sys_exit` (`93`)
    - An `ecall` is dispatched, instructing the emulator to exit.
- If the test case passes:
    - Register `a0` is set to `0`
    - Register `a7` is set to `sys_exit` (`93`)
    - An `ecall` id dispatched, instructing the emulator to exit.

A simple test runner for these tests should:
- Load the ELF file into the memory bus.
- Instantiate an emulator with the memory containing the program.
- Register a system call interupt handler that exits emulation + sets the exit code when `sys_exit` is dispatched.
- Run emulation until the program is exited by a system call interrupt.
- Assert that the exit code is `0`.

[rv-tests]: https://github.com/riscv-software-src/riscv-tests
