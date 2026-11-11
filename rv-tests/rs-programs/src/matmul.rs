//! Dense integer matrix multiplication: nested loops of loads, a multiply and an add.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;
use common::XorShift;

const N: usize = 112;

static mut A: [[u32; N]; N] = [[0; N]; N];
static mut B: [[u32; N]; N] = [[0; N]; N];
static mut C: [[u32; N]; N] = [[0; N]; N];

fn kernel() -> u64 {
    let (a, b, c) = unsafe { (&mut A, &mut B, &mut C) };
    let mut rng = XorShift(7);
    for i in 0..N {
        for j in 0..N {
            a[i][j] = rng.next() as u32;
            b[i][j] = rng.next() as u32;
        }
    }
    for i in 0..N {
        for j in 0..N {
            let mut sum = 0u32;
            for k in 0..N {
                sum = sum.wrapping_add(a[i][k].wrapping_mul(b[k][j]));
            }
            c[i][j] = sum;
        }
    }
    c.iter().flatten().fold(0u64, |acc, v| acc.rotate_left(7) ^ *v as u64)
}

entry!(kernel, 4941996873514035880);

