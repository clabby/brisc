//! The Instruction Fetch pipeline stage.

use super::register::PipelineRegister;
use crate::{
    errors::{PipelineError, PipelineResult},
    memory::Memory,
};
use cfg_if::cfg_if;

/// Execute the Instruction Fetch pipeline stage.
pub fn instruction_fetch<M: Memory>(
    p_reg: &mut PipelineRegister,
    memory: &M,
) -> PipelineResult<()> {
    // Fetch the instruction from memory at the current program counter.
    let instr_raw = memory.get_word(p_reg.pc).map_err(PipelineError::MemoryError)?;
    p_reg.instruction_raw = Some(instr_raw);

    // Increment the program counter eagerly. If a branch is taken, the program counter
    // will be updated in the `execute` stage.
    cfg_if! {
        if #[cfg(feature = "c")] {
            let inc = if brisc_isa::is_compressed(instr_raw) { 2 } else { 4 };
        } else {
            let inc = 4;
        }
    };
    p_reg.next_pc = p_reg.pc + inc;

    Ok(())
}
