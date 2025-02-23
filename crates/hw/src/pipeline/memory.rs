//! Memory read/write stage for the pipeline.

use super::register::PipelineRegister;
use crate::{
    errors::{PipelineError, PipelineResult},
    memory::Memory,
};
use brisc_isa::{
    sign_extend, Byte, DoubleWord, EnvironmentFunction, HalfWord, Instruction, LoadFunction,
    StoreFunction, Word, XWord,
};

/// Execute the Memory pipeline stage.
pub fn mem_access<M: Memory>(p_reg: &mut PipelineRegister, memory: &mut M) -> PipelineResult<()> {
    let instruction = p_reg.instruction.ok_or(PipelineError::MissingState("instruction"))?;
    let effective_address = p_reg.alu_result.ok_or(PipelineError::MissingState("alu_result"))?;

    match instruction {
        Instruction::MemoryLoad(_, funct) => {
            // Load the value from memory.
            let value = match funct {
                LoadFunction::Lb => sign_extend(
                    memory.get_byte(effective_address).map_err(PipelineError::MemoryError)?
                        as XWord,
                    7,
                ),
                LoadFunction::Lbu => {
                    memory.get_byte(effective_address).map_err(PipelineError::MemoryError)? as XWord
                }
                LoadFunction::Lh => sign_extend(
                    memory.get_halfword(effective_address).map_err(PipelineError::MemoryError)?
                        as XWord,
                    15,
                ),
                LoadFunction::Lhu => {
                    memory.get_halfword(effective_address).map_err(PipelineError::MemoryError)?
                        as XWord
                }
                LoadFunction::Lw => {
                    // TODO: cfg-if for efficiency, no need to sign extend in rv32
                    sign_extend(
                        memory.get_word(effective_address).map_err(PipelineError::MemoryError)?
                            as XWord,
                        31,
                    )
                }
                #[cfg(feature = "64-bit")]
                LoadFunction::Lwu => {
                    memory.get_word(effective_address).map_err(PipelineError::MemoryError)? as XWord
                }
                #[cfg(feature = "64-bit")]
                LoadFunction::Ld => {
                    memory.get_doubleword(effective_address).map_err(PipelineError::MemoryError)?
                }
            };
            p_reg.memory = Some(value);
        }
        Instruction::MemoryStore(_, funct) => {
            // Store the value to memory.
            let value = p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))?;
            match funct {
                StoreFunction::Sb => {
                    memory
                        .set_byte(effective_address, value as Byte)
                        .map_err(PipelineError::MemoryError)?;
                }
                StoreFunction::Sh => {
                    memory
                        .set_halfword(effective_address, value as HalfWord)
                        .map_err(PipelineError::MemoryError)?;
                }
                StoreFunction::Sw => {
                    memory
                        .set_word(effective_address, value as Word)
                        .map_err(PipelineError::MemoryError)?;
                }
                #[cfg(feature = "64-bit")]
                StoreFunction::Sd => {
                    memory
                        .set_doubleword(effective_address, value as DoubleWord)
                        .map_err(PipelineError::MemoryError)?;
                }
            }
        }
        Instruction::Environment(_, funct) => {
            if matches!(funct, EnvironmentFunction::Ecall) {
                todo!()
            }
        }
        _ => { /* no-op */ }
    }

    Ok(())
}
