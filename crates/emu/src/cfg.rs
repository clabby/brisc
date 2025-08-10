//! Emulator type configuration trait

use brisc_hw::{kernel::Kernel, memory::Memory};

/// The [`EmuConfig`] trait defines the type configuration for the emulator.
pub trait EmuConfig {
    /// The [Memory] type used by the emulator.
    type Memory: Memory;

    /// The kernel used by the emulator.
    type Kernel: Kernel;
}
