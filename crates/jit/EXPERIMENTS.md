# Experiments

What was tried while building the JIT, with what it did, so that the next attempt does not start
from nothing. Compile times were measured on an Apple M4 Max; the guest programs are those in
`rv-tests/rs-programs`. Speedups are over the interpreter, best of several runs, and the machine
and the layout of the code move small differences by a few percent.

## Kept

- **Regions of a page with internal control flow, entered at loop headers.** Compiling single
  basic blocks, or loops without their calls, left too much of the work in the interpreter and in
  the cost of entering and leaving. A region is entered at its first instruction and at every
  instruction that a jump back goes to, which is where the interpreter returns after a region has
  left. That also lets a region that runs for long be replaced by a better one (see below).
- **Following calls and returns.** Calls are written as `auipc` and `jalr`, which the compiler
  resolves because the instruction before the `jalr` set its register to a constant. A return is
  compared with the addresses after the calls in the region. `fib` went from 0.6 times the speed of
  the interpreter, because every call and return left the region, to 4 times.
- **Leaving the region for anything that is not a plain memory access.** A TLB miss, an unmapped or
  watched page, an access across pages and an error of the memory make a region leave before the
  instruction, and the interpreter executes it. Regions first called helpers for these. Leaving
  produced less code, compiled faster and left error handling in one place, with the same results.
- **A table for finding regions** instead of a hash map keyed by page and slot. Entering a region
  is frequent for loops that run few iterations, and the hash was most of its cost: `crc-rle`
  1.66 to 1.79 times and `bfvm` 1.1 to 1.55 times.
- **A separate copy of the interpreter loop with the counting of jumps back**, chosen at run
  time. Counting in the only copy slowed programs that never compile anything by 11% on average
  and by up to 22%; with two copies the interpreter is as fast as without the feature.
- **A region cap of 1500 instructions.** At 400 the loop of `sha256` was cut in two and left the
  region on each iteration, running at 0.4 times the speed of the interpreter.
- **A threshold of 2000 iterations.** At 1000, `eth-transfer`, which runs for 0.3 ms, still
  started the compiler thread and ran 7% slower.
- **Compiling quickly first and with optimizations once a region has run for long.** Compiling a
  large region takes 5 ms without optimizations and 14 to 18 ms with them, and the quick code runs
  about 40% slower. Over the guest programs the geometric mean speedup was 2.46 times with both
  tiers and 2.44 and 2.17 times with only the optimized or only the quick one; for the long runs
  it is the same as optimized only. The tiers need a way back into a region that has not left:
  a quick region leaves after 100 000 instructions.
- **Compiling on a thread of its own.** A short program does not wait for the compiler. For the
  programs here that is worth up to a factor of two where compiling takes a large part of the run.

## Rejected

- **Cranelift's single pass register allocator.** Compiles twice as fast and produces code twice
  as slow and bigger.
- **Writing the registers back in each exit instead of in a shared block.** No faster to
  compile (sha256 12.5 to 14.9 ms, quicksort 10.6 to 12.6 ms) and bigger.
- **Unchecked lookups of frames in the memory fast path of the interpreter, specialised forms of
  common instructions, and tail duplication in the interpreter.** Within noise.
- **LLVM as a replacement for Cranelift.** On regions of 500 instructions shaped like these, its
  best code is 1.3 to 1.5 times faster than Cranelift's optimized code and it takes 1.7 times (tuned)
  to 5 times as long to compile; with the static libraries that `llvm-sys` links by default it takes
  2.2 to 2.5 times as long again. At the fastest setting it compiles as fast as Cranelift without
  optimizations but its code is 2.4 times slower. Replacing Cranelift would pay off after about
  3.6e8 instructions in one region and a second tier after about 1e9. Register allocation takes
  most of the time in both compilers, because about 30 guest registers are live into every exit.

## Not tried

- **Following indirect jumps.** `bfvm`, an interpreter whose dispatch is a jump table, gains only
  1.5 times because each guest operation leaves the region. Recording the targets that an
  indirect jump goes to and compiling them into the region would help programs like it.
- **LLVM as a tier above Cranelift**, with the settings that the measurements above suggest:
  a cheap pass pipeline, code generation at the first optimization level, cold branch weights on
  the exits, and a dynamic link on macOS.
- **Fewer live registers at exits**, which would speed up compiling in both compilers.
- **Keeping compiled code between runs.**

