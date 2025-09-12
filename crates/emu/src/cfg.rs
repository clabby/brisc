//! Emulator type configuration trait

use brisc_hw::memory::Memory;

/// The [`EmuConfig`] trait defines the type configuration for the emulator.
pub trait EmuConfig<'ctx> {
    /// The [Memory] type used by the emulator.
    type Memory: Memory;

    /// The kernel used by the emulator.
    type Kernel;

    /// The external state passed to the kernel.
    type Context: 'ctx;
}
