//! A builder for the [`StEmu`] emulator.

use super::StEmu;
use crate::{cfg::EmuConfig, elf::load_elf};
use alloc::string::String;
use brisc_hw::{pipeline::PipelineRegister, XWord};

/// A builder for the [`StEmu`] emulator.
#[derive(Debug)]
pub struct StEmuBuilder<Config>
where
    Config: EmuConfig,
{
    /// The starting program counter.
    pub pc: XWord,
    /// The initial memory for the emulator.
    pub memory: Option<Config::Memory>,
    /// The system call interface for the emulator.
    pub kernel: Option<Config::Kernel>,
}

impl<Config> Default for StEmuBuilder<Config>
where
    Config: EmuConfig,
{
    fn default() -> Self {
        Self { pc: 0, memory: None, kernel: None }
    }
}

impl<Config> StEmuBuilder<Config>
where
    Config: EmuConfig,
    Config::Memory: Default,
{
    /// Loads an elf file into the emulator builder, initializing the program counter and memory.
    pub fn with_elf(mut self, elf_bytes: &[u8]) -> Result<Self, String> {
        let (memory, entry_pc) = load_elf::<Config::Memory>(elf_bytes)?;
        self.pc = entry_pc;
        self.memory = Some(memory);
        Ok(self)
    }
}

impl<Config> StEmuBuilder<Config>
where
    Config: EmuConfig,
{
    /// Assigns the entry point of the program.
    pub const fn with_pc(mut self, pc: XWord) -> Self {
        self.pc = pc;
        self
    }

    /// Assigns a pre-created memory instance to the emulator.
    pub fn with_memory(mut self, memory: Config::Memory) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Assigns the kernel to the emulator.
    pub fn with_kernel(mut self, kernel: Config::Kernel) -> Self {
        self.kernel = Some(kernel);
        self
    }

    /// Builds the emulator with the current configuration.
    ///
    /// ## Panics
    ///
    /// Panics if the memory or kernel is not set.
    pub fn build(self) -> StEmu<Config> {
        StEmu {
            register: PipelineRegister::new(self.pc),
            memory: self.memory.expect("Memory not instantiated"),
            kernel: self.kernel.expect("Kernel not instantiated"),
        }
    }
}
