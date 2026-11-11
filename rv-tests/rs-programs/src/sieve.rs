//! The sieve of Eratosthenes: byte stores at a stride, then a scan.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;

const N: usize = 1_000_000;

static mut COMPOSITE: [u8; N] = [0; N];

fn kernel() -> u64 {
    let composite = unsafe { &mut COMPOSITE };
    let mut i = 2;
    while i * i < N {
        if composite[i] == 0 {
            let mut j = i * i;
            while j < N {
                composite[j] = 1;
                j += i;
            }
        }
        i += 1;
    }
    let mut count = 0u64;
    let mut last = 0u64;
    for (n, flag) in composite.iter().enumerate().skip(2) {
        if *flag == 0 {
            count += 1;
            last = n as u64;
        }
    }
    count << 32 | last
}

entry!(kernel, 337146343801391);

