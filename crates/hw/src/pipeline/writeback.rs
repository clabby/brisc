//! The writeback pipeline stage.

use super::register::PipelineRegister;
use crate::errors::PipelineResult;

/// Execute the WriteBack pipeline stage.
pub const fn writeback(p_reg: &mut PipelineRegister) -> PipelineResult<()> {
    if let Some(rd) = p_reg.rd {
        // No-op illegal writes to the zero register.
        if rd == 0 {
            return Ok(());
        }

        // Store the result in the destination register.
        if let Some(mem) = p_reg.memory {
            p_reg.registers[rd as usize] = mem;
        } else if let Some(alu_result) = p_reg.alu_result {
            p_reg.registers[rd as usize] = alu_result;
        }
    }

    Ok(())
}
