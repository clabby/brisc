//! 5-stage RISC-V pipeline stages and state.

mod fetch;
pub use fetch::instruction_fetch;

mod decode;
pub use decode::decode_instruction;

mod execute;
pub use execute::execute;

mod memory;
pub use memory::mem_access;

mod writeback;
pub use writeback::writeback;

mod register;
pub use register::PipelineRegister;
