//! A table-driven CRC-32 and a run-length encoder over a byte buffer with runs: byte loops with
//! table lookups and data-dependent branches.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;
use common::XorShift;

static mut TABLE: [u32; 256] = [0; 256];
static mut DATA: [u8; 1 << 18] = [0; 1 << 18];
static mut OUT: [u8; 1 << 19] = [0; 1 << 19];

fn kernel() -> u64 {
    let (table, data, out) = unsafe { (&mut TABLE, &mut DATA, &mut OUT) };
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }

    let mut rng = XorShift(99);
    let mut i = 0;
    while i < data.len() {
        let run = 1 + (rng.next() & 7) as usize;
        let byte = rng.next() as u8;
        for slot in data[i..].iter_mut().take(run) {
            *slot = byte;
        }
        i += run;
    }

    let mut crc = !0u32;
    for byte in data.iter() {
        crc = table[((crc ^ *byte as u32) & 0xff) as usize] ^ (crc >> 8);
    }

    let mut written = 0;
    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        let mut run = 1;
        while i + run < data.len() && data[i + run] == byte && run < 255 {
            run += 1;
        }
        out[written] = run as u8;
        out[written + 1] = byte;
        written += 2;
        i += run;
    }
    (!crc as u64) << 32 | written as u64
}

entry!(kernel, 17197608910761084030);

