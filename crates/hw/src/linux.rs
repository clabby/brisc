//! Linux system call interface.

use crate::{errors::PipelineResult, memory::Memory, pipeline::PipelineRegister};
use brisc_isa::XWord;

/// The [SyscallInterface] trait defines the interface for performing system calls.
pub trait SyscallInterface {
    /// Perform a system call with the given arguments.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
    ) -> PipelineResult<XWord>;
}

impl SyscallInterface for () {
    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
    ) -> PipelineResult<XWord> {
        unimplemented!()
    }
}
