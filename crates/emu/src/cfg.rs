//! Emulator type configuration trait

use crate::kernel::AsyncKernel;
use brisc_hw::bus::Bus;

/// The [`EmuConfig`] trait defines the type configuration for the emulator.
///
/// The `'ctx` lifetime lets a configuration name a context that borrows, such as
/// `&'ctx mut State`, without carrying a lifetime parameter itself.
pub trait EmuConfig<'ctx> {
    /// The [Bus] that the hart runs on: its memory and devices.
    type Bus: Bus;

    /// The kernel used by the emulator.
    ///
    /// Every [Kernel] qualifies, because it is also an [AsyncKernel].
    ///
    /// [Kernel]: crate::kernel::Kernel
    type Kernel: AsyncKernel<Self::Context> + 'ctx;

    /// The external state passed to the kernel.
    type Context: 'ctx;
}
