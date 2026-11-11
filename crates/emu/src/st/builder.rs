//! A builder for the [`StEmu`] emulator.

use super::{BusError, StEmu};
use crate::{
    cfg::EmuConfig,
    elf::{load_elf, LoadError},
};
use brisc_hw::{cpu::Cpu, XWord};

/// A builder for the [`StEmu`] emulator.
#[derive(Debug)]
pub struct StEmuBuilder<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    /// The starting program counter.
    pub pc: XWord,
    /// The bus of the emulator.
    pub bus: Option<Config::Bus>,
    /// The system call interface for the emulator.
    pub kernel: Option<Config::Kernel>,
    /// The emulator's state.
    pub state: Option<Config::Context>,
}

impl<'ctx, Config> Default for StEmuBuilder<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    fn default() -> Self {
        Self { pc: 0, bus: None, kernel: None, state: None }
    }
}

impl<'ctx, Config> StEmuBuilder<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
    Config::Bus: Default,
{
    /// Loads an ELF file and sets the program counter to its entry point.
    ///
    /// The file is loaded onto the bus assigned with [StEmuBuilder::with_bus], or onto a
    /// default one if none has been assigned.
    pub fn with_elf(mut self, elf_bytes: &[u8]) -> Result<Self, LoadError<BusError<'ctx, Config>>> {
        let mut bus = self.bus.take().unwrap_or_default();
        self.pc = load_elf(&mut bus, elf_bytes)?;
        self.bus = Some(bus);
        Ok(self)
    }
}

impl<'ctx, Config> StEmuBuilder<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    /// Assigns the entry point of the program.
    pub const fn with_pc(mut self, pc: XWord) -> Self {
        self.pc = pc;
        self
    }

    /// Assigns the bus to the emulator.
    pub fn with_bus(mut self, bus: Config::Bus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Assigns the kernel to the emulator.
    pub fn with_kernel(mut self, kernel: Config::Kernel) -> Self {
        self.kernel = Some(kernel);
        self
    }

    /// Assigns the state to the emulator.
    pub fn with_ctx(mut self, state: Config::Context) -> Self {
        self.state = Some(state);
        self
    }

    /// Builds the emulator with the current configuration.
    ///
    /// ## Panics
    ///
    /// Panics if the bus, kernel or state is not set.
    pub fn build(self) -> StEmu<'ctx, Config> {
        StEmu {
            cpu: Cpu::new(self.pc),
            bus: self.bus.expect("Bus not instantiated"),
            kernel: self.kernel.expect("Kernel not instantiated"),
            ctx: self.state.expect("State not instantiated"),
        }
    }
}
