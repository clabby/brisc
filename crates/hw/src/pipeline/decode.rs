//! The Instruction Decode pipeline stage.

use super::register::PipelineRegister;
use crate::errors::{PipelineError, PipelineResult};
use brisc_isa::{Instruction, REG_A7};

/// Executes the instruction fetch stage.
///
/// In this stage, the raw instruction is decoded, the register values are read from the
/// register file, and then finally copied into the [PipelineRegister].
pub fn decode_instruction(register: &mut PipelineRegister) -> PipelineResult<()> {
    // Decode the raw instruction.
    let instruction_raw =
        register.instruction_raw.ok_or(PipelineError::MissingState("instruction_raw"))?;
    let instruction = Instruction::try_from(instruction_raw)?;

    // Read register values and update the stage state.
    register.rs1_value = instruction.rs1().map(|rs1| register.registers[rs1 as usize]);
    register.rs2_value = instruction.rs2().map(|rs2| register.registers[rs2 as usize]);
    register.rd = instruction.rd();
    register.immediate = instruction.immediate();

    // Set the decoded instruction in the pipeline register.
    register.instruction = Some(instruction);

    // Throw an interrupt if the instruction is a system call.
    if instruction.is_system_call() {
        return Err(PipelineError::SyscallException(register.registers[REG_A7 as usize]));
    }

    Ok(())
}
