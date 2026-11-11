//! The Mandelbrot set in fixed-point arithmetic: a multiply-heavy loop with a data-dependent exit.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;

fn kernel() -> u64 {
    const SIZE: i64 = 120;
    const FRACTION: u32 = 24;
    let mut checksum = 0u64;
    for py in 0..SIZE {
        for px in 0..SIZE {
            let cr = ((px * 3 << FRACTION) / SIZE) - (2 << FRACTION);
            let ci = ((py * 2 << FRACTION) / SIZE) - (1 << FRACTION);
            let (mut zr, mut zi) = (0i64, 0i64);
            let mut iterations = 0u64;
            while iterations < 96 {
                let (zr2, zi2) = (zr * zr >> FRACTION, zi * zi >> FRACTION);
                if zr2 + zi2 > 4 << FRACTION {
                    break;
                }
                zi = (2 * zr * zi >> FRACTION) + ci;
                zr = zr2 - zi2 + cr;
                iterations += 1;
            }
            checksum = checksum.wrapping_mul(131).wrapping_add(iterations);
        }
    }
    checksum
}

entry!(kernel, 10103963163211582525);

