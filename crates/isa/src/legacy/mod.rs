//! The decoder that [Instruction](crate::Instruction) replaces.

#![allow(clippy::missing_const_for_fn)]

mod functions;
pub use functions::*;

mod instructions;
pub use instructions::*;

#[cfg(test)]
mod differential;
