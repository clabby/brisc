//! Linux kernel interface.

use crate::{errors::PipelineResult, memory::Memory, pipeline::PipelineRegister};
use brisc_isa::XWord;

/// The [`Kernel`] trait defines the interface for performing system calls.
pub trait Kernel<S> {
    /// Perform a system call with the given arguments.
    #[cfg(not(feature = "async-kernel"))]
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
        state: &mut S,
    ) -> PipelineResult<XWord>;

    /// Perform a system call with the given arguments.
    #[cfg(feature = "async-kernel")]
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
        state: &mut S,
    ) -> impl core::future::Future<Output = PipelineResult<XWord>>;
}

impl<S> Kernel<S> for () {
    #[cfg(not(feature = "async-kernel"))]
    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
        _: &mut S,
    ) -> PipelineResult<XWord> {
        unimplemented!()
    }

    #[cfg(feature = "async-kernel")]
    async fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
        _: &mut S,
    ) -> PipelineResult<XWord> {
        unimplemented!()
    }
}
