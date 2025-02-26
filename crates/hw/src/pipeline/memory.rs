//! Memory read/write stage for the pipeline.

use super::register::PipelineRegister;
use crate::{
    errors::{PipelineError, PipelineResult},
    memory::Memory,
};
use brisc_isa::{
    sign_extend, Byte, HalfWord, Instruction, LoadFunction, StoreFunction, Word, XWord,
};

#[cfg(feature = "64-bit")]
use brisc_isa::DoubleWord;

#[cfg(feature = "a")]
use brisc_isa::{AmoFunction, SXWord};

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
                LoadFunction::Lw => sign_extend(
                    memory.get_word(effective_address).map_err(PipelineError::MemoryError)?
                        as XWord,
                    31,
                ),
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
        #[cfg(feature = "a")]
        Instruction::Amo(r_type, funct) => {
            // Perform atomic memory operation.
            let size = 1 << r_type.funct3;
            if size < 4 {
                return Err(PipelineError::BadAmoSize(size));
            }

            let addr = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))?;
            if size == 8 && addr & 7 != 0 || size == 4 && addr & 3 != 0 {
                return Err(PipelineError::UnalignedAmo);
            }

            match funct {
                AmoFunction::Lr => {
                    let value = match size {
                        4 => memory.get_word(addr).map_err(PipelineError::MemoryError)? as XWord,
                        #[cfg(feature = "64-bit")]
                        8 => memory.get_doubleword(addr).map_err(PipelineError::MemoryError)?,
                        _ => return Err(PipelineError::BadAmoSize(size)),
                    };
                    p_reg.memory = Some(value);
                    p_reg.reservation = Some(addr);
                }
                AmoFunction::Sc => {
                    p_reg.memory = Some(1);
                    if let Some(reservation) = p_reg.reservation {
                        if reservation == addr {
                            let rs2 =
                                p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))?;

                            match size {
                                4 => memory
                                    .set_word(addr, rs2 as Word)
                                    .map_err(PipelineError::MemoryError)?,
                                #[cfg(feature = "64-bit")]
                                8 => memory
                                    .set_doubleword(addr, rs2)
                                    .map_err(PipelineError::MemoryError)?,
                                _ => return Err(PipelineError::BadAmoSize(size)),
                            }

                            p_reg.memory = Some(0);
                        }
                    }
                    p_reg.reservation = None;
                }
                instr => {
                    #[allow(unused_mut)]
                    let mut rs2 =
                        p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))?;

                    let mut mem = match size {
                        4 => memory.get_word(addr).map_err(PipelineError::MemoryError)? as XWord,
                        #[cfg(feature = "64-bit")]
                        8 => memory.get_doubleword(addr).map_err(PipelineError::MemoryError)?,
                        _ => return Err(PipelineError::BadAmoSize(size)),
                    };

                    #[cfg(feature = "64-bit")]
                    if size == 4 {
                        rs2 = sign_extend(rs2 & 0xFFFF_FFFF, 31);
                        mem = sign_extend(mem & 0xFFFF_FFFF, 31);
                    }

                    p_reg.memory = Some(mem as XWord);

                    mem = match instr {
                        AmoFunction::Amoswap => rs2,
                        AmoFunction::Amoadd => mem.wrapping_add(rs2),
                        AmoFunction::Amoxor => mem ^ rs2,
                        AmoFunction::Amoand => mem & rs2,
                        AmoFunction::Amoor => mem | rs2,
                        AmoFunction::Amomin => (rs2 as SXWord).min(mem as SXWord) as XWord,
                        AmoFunction::Amomax => (rs2 as SXWord).max(mem as SXWord) as XWord,
                        AmoFunction::Amominu => rs2.min(mem),
                        AmoFunction::Amomaxu => rs2.max(mem),
                        _ => unreachable!(),
                    };

                    if size == 4 {
                        memory.set_word(addr, mem as Word).map_err(PipelineError::MemoryError)?;
                    } else {
                        memory
                            .set_doubleword(addr, mem as brisc_isa::DoubleWord)
                            .map_err(PipelineError::MemoryError)?;
                    }
                }
            }
        }
        _ => { /* no-op */ }
    }

    Ok(())
}
