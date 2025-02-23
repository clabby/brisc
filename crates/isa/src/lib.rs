#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations, unreachable_pub, rustdoc::all)]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(not(test), no_std)]

mod bits;
pub use bits::sign_extend;

mod errors;
pub use errors::InstructionDecodeError;

mod instructions;
pub use instructions::*;

mod functions;
pub use functions::*;

mod arch;
pub use arch::*;
