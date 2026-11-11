//! Emulator type configuration trait

use crate::kernel::AsyncKernel;
use brisc_hw::memory::Memory;

/// The [`EmuConfig`] trait defines the type configuration for the emulator.
///
/// The `'ctx` lifetime lets a configuration name a context that borrows, such as
/// `&'ctx mut State`, without carrying a lifetime parameter itself.
pub trait EmuConfig<'ctx> {
    /// The [Memory] type used by the emulator.
    type Memory: Memory;

    /// The kernel used by the emulator.
    ///
    /// Every [Kernel] qualifies, because it is also an [AsyncKernel].
    ///
    /// [Kernel]: crate::kernel::Kernel
    type Kernel: AsyncKernel<Self::Context> + 'ctx;

    /// The external state passed to the kernel.
    type Context: 'ctx;
}
