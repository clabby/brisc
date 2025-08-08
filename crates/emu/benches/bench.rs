//! Benchmarking for the `brisc-emu` crate.

use criterion::criterion_main;

mod rs_programs;

criterion_main!(rs_programs::benches);
