//! Linux kernel interface.

use crate::{memory::Memory, pipeline::PipelineRegister};
use brisc_isa::XWord;

/// The [`Kernel`] trait defines the interface for performing system calls.
pub trait Kernel<S> {
    /// The error type returned by the kernel.
    type Error;

    /// Perform a system call with the given arguments.
    #[cfg(not(feature = "async-kernel"))]
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
        state: &mut S,
    ) -> Result<XWord, Self::Error>;

    /// Perform a system call with the given arguments.
    #[cfg(feature = "async-kernel")]
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        p_reg: &mut PipelineRegister,
        state: &mut S,
    ) -> impl core::future::Future<Output = Result<XWord, Self::Error>>;
}

impl<S> Kernel<S> for () {
    type Error = ();

    #[cfg(not(feature = "async-kernel"))]
    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
        _: &mut S,
    ) -> Result<XWord, Self::Error> {
        unimplemented!()
    }

    #[cfg(feature = "async-kernel")]
    async fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut PipelineRegister,
        _: &mut S,
    ) -> Result<XWord, Self::Error> {
        unimplemented!()
    }
}
