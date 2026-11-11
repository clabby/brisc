//! A binary search tree in an arena: inserts and lookups that chase pointers and branch on
//! comparisons.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![allow(static_mut_refs, dead_code)]

#[macro_use]
mod common;
use common::XorShift;

const NODES: usize = 1 << 15;

#[derive(Clone, Copy)]
struct Node {
    key: u64,
    left: u32,
    right: u32,
}

static mut ARENA: [Node; NODES + 1] = [Node { key: 0, left: 0, right: 0 }; NODES + 1];

fn kernel() -> u64 {
    let arena = unsafe { &mut ARENA };
    let mut rng = XorShift(0xdead_beef);
    // Index 0 is the null link; index 1 is the root.
    arena[1].key = rng.next();
    let mut len = 2;
    while len <= NODES {
        let key = rng.next();
        let mut at = 1;
        loop {
            let node = &mut arena[at];
            let next = if key < node.key { &mut node.left } else { &mut node.right };
            if *next == 0 {
                *next = len as u32;
                break;
            }
            at = *next as usize;
        }
        arena[len].key = key;
        len += 1;
    }

    let mut rng = XorShift(0xdead_beef);
    let mut checksum = 0u64;
    for _ in 0..2 {
        for _ in 0..NODES {
            let key = rng.next();
            let (mut at, mut depth) = (1usize, 0u64);
            while arena[at].key != key && at != 0 {
                at = if key < arena[at].key { arena[at].left } else { arena[at].right } as usize;
                depth += 1;
            }
            checksum = checksum.wrapping_mul(31).wrapping_add(depth);
        }
        rng = XorShift(0xdead_beef);
    }
    checksum
}

entry!(kernel, 13436856606672526032);

