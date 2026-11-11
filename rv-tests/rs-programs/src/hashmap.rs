//! An open-addressing hash table filled with pseudo-random keys and then probed: random memory
//! access, a multiplicative hash and short unpredictable probe loops.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;
use common::XorShift;

const SLOTS: usize = 1 << 17;

static mut KEYS: [u64; SLOTS] = [0; SLOTS];
static mut VALUES: [u64; SLOTS] = [0; SLOTS];

#[inline(always)]
fn slot(key: u64) -> usize {
    (key.wrapping_mul(0x9e3779b97f4a7c15) >> 47) as usize & (SLOTS - 1)
}

fn kernel() -> u64 {
    let (keys, values) = unsafe { (&mut KEYS, &mut VALUES) };
    let mut rng = XorShift(12345);
    for i in 0..(SLOTS * 3 / 4) as u64 {
        let key = rng.next() | 1;
        let mut s = slot(key);
        while keys[s] != 0 && keys[s] != key {
            s = (s + 1) & (SLOTS - 1);
        }
        keys[s] = key;
        values[s] = i;
    }

    let mut rng = XorShift(12345);
    let mut checksum = 0u64;
    for round in 0..3u64 {
        for _ in 0..(SLOTS * 3 / 4) {
            let key = rng.next() | 1;
            let mut s = slot(key);
            while keys[s] != 0 && keys[s] != key {
                s = (s + 1) & (SLOTS - 1);
            }
            checksum = checksum.wrapping_add(values[s] ^ round);
        }
        rng = XorShift(12345);
    }
    checksum
}

entry!(kernel, 14495367168);

