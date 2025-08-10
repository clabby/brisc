//! Benches for the [`StEmu`] on Rust programs.
//!
//! [`StEmu`]: crate::st::StEmu

#![allow(unused, missing_docs)]

use criterion::{criterion_group, Criterion};
use std::fs;

/// Benchmarks for the RISC-V Rust programs in the `rv-tests` directory.
fn bench_rs_programs(c: &mut Criterion) {
    let programs = fs::read_dir("../../rv-tests/bin")
        .expect("Failed to read rs_programs directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("rs-64"))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    #[cfg(feature = "test-utils")]
    for program in programs {
        let program_name = program.file_name().unwrap().to_string_lossy();
        c.bench_function(&format!("emu/rs_programs/{program_name}"), |b| {
            b.iter(|| {
                brisc_emu::test_utils::run_riscv_test(&program);
            });
        });
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_rs_programs
}
