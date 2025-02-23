//! Emulator configuration trait.

use brisc_hw::{linux::SyscallInterface, memory::Memory};

#[allow(unused, missing_docs)]
pub trait EmuConfig {
    /// The [Memory] type used by the emulator.
    type Memory: Memory;

    /// The system call interface used by the emulator.
    type SyscallInterface: SyscallInterface;

    /// External configuration for the emulator.
    type ExternalConfig;
}
