//! Linux kernel interface.

use crate::{errors::PipelineResult, memory::Memory, pipeline::PipelineRegister};
use brisc_isa::XWord;

/// The [`Kernel`] trait defines the interface for performing system calls.
pub trait Kernel {
    /// Perform a system call with the given arguments.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
    ) -> PipelineResult<XWord>;
}

impl Kernel for () {
    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
    ) -> PipelineResult<XWord> {
        unimplemented!()
    }
}
