#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations, unreachable_pub, rustdoc::all)]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(not(any(test, feature = "test-utils")), no_std)]

extern crate alloc;

mod budget;
pub use budget::Budget;

pub mod elf;
pub mod kernel;

mod emulator;
pub use emulator::{EmulationError, Emulator, Parts};

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
