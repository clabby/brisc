//! The instruction execution stage of the pipeline.

use super::register::PipelineRegister;
use crate::errors::{PipelineError, PipelineResult};
use brisc_isa::{
    BType, BranchFunction, EnvironmentFunction, IType, ImmediateArithmeticFunction, Instruction,
    RegisterArithmeticFunction, SXWord, XWord, SHIFT_MASK,
};

#[cfg(feature = "64-bit")]
use brisc_isa::{
    sign_extend, ImmediateArithmeticWordFunction, RegisterArithmeticWordFunction, Word,
};

#[cfg(feature = "m")]
use brisc_isa::{DoubleXWord, X_LEN};

/// Execute the ALU stage of the pipeline.
pub fn execute(p_reg: &mut PipelineRegister) -> PipelineResult<()> {
    let instruction = p_reg.instruction.ok_or(PipelineError::MissingState("instruction"))?;

    let result = match instruction {
        Instruction::MemoryLoad(_, _) | Instruction::MemoryStore(_, _) => execute_mem(p_reg)?,
        Instruction::Branch(b_type, funct) => {
            let target = execute_branch(p_reg, b_type, funct)?;
            p_reg.next_pc = target;
            target
        }
        Instruction::ImmediateArithmetic(i_type, funct) => {
            execute_imm_arithmetic(p_reg, i_type, funct)?
        }
        Instruction::RegisterArithmetic(_, funct) => execute_reg_arithmetic(p_reg, funct)?,
        Instruction::Lui(u_type) => u_type.imm,
        Instruction::Auipc(u_type) => p_reg.pc + u_type.imm,
        Instruction::Jal(j_type) => {
            let result = p_reg.next_pc;
            p_reg.next_pc = p_reg.pc + j_type.imm;
            result
        }
        Instruction::Jalr(i_type) => {
            let result = p_reg.next_pc;
            let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))?;
            p_reg.next_pc = (rs1 + i_type.imm) & !1;
            result
        }
        Instruction::Fence => {
            // no-op FENCE operations. This emulator only supports a single RISC-V hart.
            0
        }
        Instruction::Environment(_i_type, funct) => {
            if matches!(funct, EnvironmentFunction::Ecall) {
                // TODO: Fix; Needs to be a0
                return Err(PipelineError::SyscallException(p_reg.registers[0]));
            } else {
                // no-op EBREAK operations.
                0
            }
        }
        #[cfg(feature = "64-bit")]
        Instruction::ImmediateArithmeticWord(i_type, funct) => {
            execute_imm_arithmetic_word(p_reg, i_type, funct)?
        }
        #[cfg(feature = "64-bit")]
        Instruction::RegisterArithmeticWord(_, funct) => execute_reg_arithmetic_word(p_reg, funct)?,
        #[cfg(feature = "a")]
        Instruction::Amo(_, _) => 0,
    };

    p_reg.alu_result = Some(result);
    Ok(())
}

/// Executes a [MemoryLoad] or [MemoryStore] instruction.
///
/// [MemoryLoad]: brisc_isa::Instruction::MemoryLoad
/// [MemoryStore]:brisc_isa::Instruction::MemoryStore
#[inline(always)]
fn execute_mem(p_reg: &PipelineRegister) -> PipelineResult<XWord> {
    p_reg.effective_address().ok_or(PipelineError::MissingState("effective_address"))
}

/// Executes a [BranchFunction] instruction, returning the target address.
#[inline(always)]
fn execute_branch(
    p_reg: &PipelineRegister,
    b_type: BType,
    funct: BranchFunction,
) -> PipelineResult<XWord> {
    let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))?;
    let rs2 = p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))?;
    let target = p_reg.pc + b_type.imm;

    match funct {
        BranchFunction::Beq => {
            if rs1 == rs2 {
                return Ok(target);
            }
        }
        BranchFunction::Bne => {
            if rs1 != rs2 {
                return Ok(target);
            }
        }
        BranchFunction::Blt => {
            if (rs1 as SXWord) < (rs2 as SXWord) {
                return Ok(target);
            }
        }
        BranchFunction::Bge => {
            if (rs1 as SXWord) >= (rs2 as SXWord) {
                return Ok(target);
            }
        }
        BranchFunction::Bltu => {
            if rs1 < rs2 {
                return Ok(target);
            }
        }
        BranchFunction::Bgeu => {
            if rs1 >= rs2 {
                return Ok(target);
            }
        }
    }

    Ok(p_reg.next_pc)
}

/// Executes an [ImmediateArithmeticFunction] instruction.
#[inline(always)]
fn execute_imm_arithmetic(
    p_reg: &PipelineRegister,
    i_type: IType,
    funct: ImmediateArithmeticFunction,
) -> PipelineResult<XWord> {
    let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))?;

    let res = match funct {
        ImmediateArithmeticFunction::Addi => i_type.imm.wrapping_add(rs1),
        ImmediateArithmeticFunction::Xori => i_type.imm ^ rs1,
        ImmediateArithmeticFunction::Ori => i_type.imm | rs1,
        ImmediateArithmeticFunction::Andi => i_type.imm & rs1,
        ImmediateArithmeticFunction::Slli => rs1 << (i_type.imm & SHIFT_MASK),
        ImmediateArithmeticFunction::Srli => rs1 >> (i_type.imm & SHIFT_MASK),
        ImmediateArithmeticFunction::Srai => (rs1 as SXWord >> (i_type.imm & SHIFT_MASK)) as XWord,
        ImmediateArithmeticFunction::Slti => ((rs1 as SXWord) < (i_type.imm as SXWord)) as XWord,
        ImmediateArithmeticFunction::Sltiu => (rs1 < i_type.imm) as XWord,
    };

    Ok(res)
}

/// Executes a [RegisterArithmeticFunction] instruction.
#[inline(always)]
fn execute_reg_arithmetic(
    p_reg: &PipelineRegister,
    funct: RegisterArithmeticFunction,
) -> PipelineResult<XWord> {
    let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))?;
    let rs2 = p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))?;

    let result = match funct {
        RegisterArithmeticFunction::Add => rs1.wrapping_add(rs2),
        RegisterArithmeticFunction::Sub => rs1.wrapping_sub(rs2),
        RegisterArithmeticFunction::Xor => rs1 ^ rs2,
        RegisterArithmeticFunction::Or => rs1 | rs2,
        RegisterArithmeticFunction::And => rs1 & rs2,
        RegisterArithmeticFunction::Sll => rs1 << (rs2 & SHIFT_MASK),
        RegisterArithmeticFunction::Srl => rs1 >> (rs2 & SHIFT_MASK),
        RegisterArithmeticFunction::Sra => (rs1 as SXWord >> (rs2 & SHIFT_MASK)) as XWord,
        RegisterArithmeticFunction::Slt => {
            if (rs1 as SXWord) < (rs2 as SXWord) {
                1
            } else {
                0
            }
        }
        RegisterArithmeticFunction::Sltu => {
            if rs1 < rs2 {
                1
            } else {
                0
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Mul => rs1 * rs2,
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Mulh => {
            let rs1 = crate::sign_extend(rs1 as DoubleXWord, (X_LEN - 1) as DoubleXWord);
            let rs2 = crate::sign_extend(rs2 as DoubleXWord, (X_LEN - 1) as DoubleXWord);
            #[cfg(not(feature = "64-bit"))]
            let result = (rs1 as i64).wrapping_mul(rs2 as i64);
            #[cfg(feature = "64-bit")]
            let result = (rs1 as i128).wrapping_mul(rs2 as i128);
            (result >> X_LEN) as XWord
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Mulhsu => {
            let rs1 = crate::sign_extend(rs1 as DoubleXWord, (X_LEN - 1) as DoubleXWord);
            #[cfg(not(feature = "64-bit"))]
            let result = (rs1 as i64).wrapping_mul(rs2 as u64 as i64);
            #[cfg(feature = "64-bit")]
            let result = (rs1 as i128).wrapping_mul(rs2 as i128);
            (result >> X_LEN) as XWord
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Mulhu => {
            #[cfg(not(feature = "64-bit"))]
            let result = (rs1 as u64) * (rs2 as u64);
            #[cfg(feature = "64-bit")]
            let result = (rs1 as u128) * (rs2 as u128);
            (result >> X_LEN) as XWord
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Div => {
            if rs2 == 0 {
                XWord::MAX
            } else {
                ((rs1 as SXWord).wrapping_div(rs2 as SXWord)) as XWord
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Divu => {
            if rs2 == 0 {
                XWord::MAX
            } else {
                rs1 / rs2
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Rem => {
            if rs2 == 0 {
                rs1
            } else {
                ((rs1 as SXWord).wrapping_rem(rs2 as SXWord)) as XWord
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticFunction::Remu => {
            if rs2 == 0 {
                rs1
            } else {
                rs1 % rs2
            }
        }
    };

    Ok(result)
}

/// Executes an [ImmediateArithmeticWordFunction] instruction.
#[cfg(feature = "64-bit")]
#[inline(always)]
fn execute_imm_arithmetic_word(
    p_reg: &PipelineRegister,
    i_type: IType,
    funct: ImmediateArithmeticWordFunction,
) -> PipelineResult<XWord> {
    let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))? as Word;

    let result = match funct {
        ImmediateArithmeticWordFunction::Addiw => (i_type.imm as Word) + rs1,
        ImmediateArithmeticWordFunction::Slliw => rs1 << (i_type.imm & 0x1F),
        ImmediateArithmeticWordFunction::Srliw => rs1 >> (i_type.imm & 0x1F),
        ImmediateArithmeticWordFunction::Sraiw => ((rs1 as i32) >> (i_type.imm & 0x1F)) as Word,
    };

    Ok(sign_extend(result as XWord, 31))
}

/// Executes a [RegisterArithmeticWordFunction] instruction.
#[cfg(feature = "64-bit")]
#[inline(always)]
fn execute_reg_arithmetic_word(
    p_reg: &PipelineRegister,
    funct: RegisterArithmeticWordFunction,
) -> PipelineResult<XWord> {
    let rs1 = p_reg.rs1_value.ok_or(PipelineError::MissingState("rs1_value"))? as Word;
    let rs2 = p_reg.rs2_value.ok_or(PipelineError::MissingState("rs2_value"))? as Word;

    let result = match funct {
        RegisterArithmeticWordFunction::Addw => rs1.wrapping_add(rs2),
        RegisterArithmeticWordFunction::Subw => rs1.wrapping_sub(rs2),
        RegisterArithmeticWordFunction::Sllw => rs1 << (rs2 & SHIFT_MASK as Word),
        RegisterArithmeticWordFunction::Srlw => rs1 >> (rs2 & SHIFT_MASK as Word),
        RegisterArithmeticWordFunction::Sraw => {
            ((rs1 as i32) >> (rs2 & SHIFT_MASK as Word)) as Word
        }
        #[cfg(feature = "m")]
        RegisterArithmeticWordFunction::Mulw => {
            let result = (rs1 as i32 as i64) * (rs2 as i32 as i64);
            result as Word
        }
        #[cfg(feature = "m")]
        RegisterArithmeticWordFunction::Divw => {
            if rs2 == 0 {
                Word::MAX
            } else {
                ((rs1 as i32).wrapping_div(rs2 as i32)) as Word
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticWordFunction::Divuw => {
            if rs2 == 0 {
                Word::MAX
            } else {
                rs1 / rs2
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticWordFunction::Remw => {
            if rs2 == 0 {
                rs1
            } else {
                ((rs1 as i32).wrapping_rem(rs2 as i32)) as Word
            }
        }
        #[cfg(feature = "m")]
        RegisterArithmeticWordFunction::Remuw => {
            if rs2 == 0 {
                rs1
            } else {
                rs1 % rs2
            }
        }
    };

    Ok(sign_extend(result as XWord, 31))
}
