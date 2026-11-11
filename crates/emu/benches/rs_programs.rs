//! Benches for the [`StEmu`] on Rust programs.
//!
//! [`StEmu`]: brisc_emu::st::StEmu

use brisc_emu::test_utils::TestProgram;
use criterion::{criterion_group, BatchSize, Criterion, Throughput};
use std::fs;

/// Benchmarks the Rust programs in `rv-tests/bin` built for the enabled word size.
///
/// Each program gets a `run` benchmark that excludes ELF loading and reports guest instructions
/// per second, and a `load` benchmark that measures ELF loading alone.
fn bench_rs_programs(c: &mut Criterion) {
    // The programs are compiled for `rv{32,64}imac`.
    if !cfg!(all(feature = "m", feature = "a", feature = "c")) {
        return;
    }
    let prefix = if cfg!(feature = "64-bit") { "rs-64bit-" } else { "rs-32bit-" };

    let mut programs = fs::read_dir("../../rv-tests/bin")
        .expect("Failed to read rv-tests/bin")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .collect::<Vec<_>>();
    programs.sort();

    for program in programs {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let elf = fs::read(&program).unwrap();
        let instructions = TestProgram::load(&elf).run();

        let mut group = c.benchmark_group(format!("emu/{name}"));
        group.throughput(Throughput::Elements(instructions));
        group.bench_function("run", |b| {
            b.iter_batched(|| TestProgram::load(&elf), TestProgram::run, BatchSize::LargeInput)
        });
        group.throughput(Throughput::Bytes(elf.len() as u64));
        group.bench_function("load", |b| b.iter(|| TestProgram::load(&elf)));
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_rs_programs
}
