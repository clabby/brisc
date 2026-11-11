//! Lowering of decoded RISC-V instructions to [Op]s.

use super::{
    op::{narrow, Op, WIDE},
    Fault, Registers,
};
use brisc_isa::{
    BranchFunction, EnvironmentFunction, ImmediateArithmeticFunction, Instruction, LoadFunction,
    RegisterArithmeticFunction, StoreFunction, Word, XWord,
};

#[cfg(feature = "64-bit")]
use brisc_isa::{ImmediateArithmeticWordFunction, RegisterArithmeticWordFunction};

#[cfg(feature = "a")]
use brisc_isa::AmoFunction;

/// Decodes the instruction in the low bits of `raw`.
pub(super) fn lower(raw: Word) -> Result<Op, Fault> {
    let instruction = Instruction::try_from(raw)?;

    // An instruction is two bytes long unless its two low bits are set.
    let length = if raw & 0b11 == 0b11 { WIDE } else { 0 };
    let op = |code: u8, rd: u8, rs1: u8, rs2: u8, imm: XWord| Op {
        code: code | length,
        rd: if rd == 0 { Registers::SINK } else { rd },
        rs1,
        rs2,
        imm: imm as i32,
    };

    Ok(match instruction {
        Instruction::MemoryLoad(i, function) => {
            let code = match function {
                LoadFunction::Lb => narrow::LB,
                LoadFunction::Lh => narrow::LH,
                LoadFunction::Lw => narrow::LW,
                LoadFunction::Lbu => narrow::LBU,
                LoadFunction::Lhu => narrow::LHU,
                #[cfg(feature = "64-bit")]
                LoadFunction::Lwu => narrow::LWU,
                #[cfg(feature = "64-bit")]
                LoadFunction::Ld => narrow::LD,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        Instruction::MemoryStore(s, function) => {
            let code = match function {
                StoreFunction::Sb => narrow::SB,
                StoreFunction::Sh => narrow::SH,
                StoreFunction::Sw => narrow::SW,
                #[cfg(feature = "64-bit")]
                StoreFunction::Sd => narrow::SD,
            };
            op(code, 0, s.rs1, s.rs2, s.imm)
        }
        Instruction::Branch(b, function) => {
            let code = match function {
                BranchFunction::Beq => narrow::BEQ,
                BranchFunction::Bne => narrow::BNE,
                BranchFunction::Blt => narrow::BLT,
                BranchFunction::Bge => narrow::BGE,
                BranchFunction::Bltu => narrow::BLTU,
                BranchFunction::Bgeu => narrow::BGEU,
            };
            op(code, 0, b.rs1, b.rs2, b.imm)
        }
        Instruction::ImmediateArithmetic(i, function) => {
            let code = match function {
                ImmediateArithmeticFunction::Addi => narrow::ADDI,
                ImmediateArithmeticFunction::Xori => narrow::XORI,
                ImmediateArithmeticFunction::Ori => narrow::ORI,
                ImmediateArithmeticFunction::Andi => narrow::ANDI,
                ImmediateArithmeticFunction::Slli => narrow::SLLI,
                ImmediateArithmeticFunction::Srli => narrow::SRLI,
                ImmediateArithmeticFunction::Srai => narrow::SRAI,
                ImmediateArithmeticFunction::Slti => narrow::SLTI,
                ImmediateArithmeticFunction::Sltiu => narrow::SLTIU,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        Instruction::RegisterArithmetic(r, function) => {
            let code = match function {
                RegisterArithmeticFunction::Add => narrow::ADD,
                RegisterArithmeticFunction::Sub => narrow::SUB,
                RegisterArithmeticFunction::Xor => narrow::XOR,
                RegisterArithmeticFunction::Or => narrow::OR,
                RegisterArithmeticFunction::And => narrow::AND,
                RegisterArithmeticFunction::Sll => narrow::SLL,
                RegisterArithmeticFunction::Srl => narrow::SRL,
                RegisterArithmeticFunction::Sra => narrow::SRA,
                RegisterArithmeticFunction::Slt => narrow::SLT,
                RegisterArithmeticFunction::Sltu => narrow::SLTU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mul => narrow::MUL,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulh => narrow::MULH,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhsu => narrow::MULHSU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhu => narrow::MULHU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Div => narrow::DIV,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Divu => narrow::DIVU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Rem => narrow::REM,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Remu => narrow::REMU,
            };
            op(code, r.rd, r.rs1, r.rs2, 0)
        }
        Instruction::Lui(u) => op(narrow::LI, u.rd, 0, 0, u.imm),
        Instruction::Auipc(u) => op(narrow::AUIPC, u.rd, 0, 0, u.imm),
        Instruction::Jal(j) => op(narrow::JAL, j.rd, 0, 0, j.imm),
        Instruction::Jalr(i) => op(narrow::JALR, i.rd, i.rs1, 0, i.imm),
        Instruction::Environment(_, EnvironmentFunction::Ecall) => op(narrow::ECALL, 0, 0, 0, 0),
        // Breakpoints and control and status registers are not modelled: the instruction only
        // clears its destination register.
        Instruction::Environment(i, EnvironmentFunction::Ebreak) => op(narrow::LI, i.rd, 0, 0, 0),
        // With a single hart and decoded code kept coherent with memory, there is nothing to
        // order or flush.
        Instruction::Fence => op(narrow::LI, 0, 0, 0, 0),
        #[cfg(feature = "64-bit")]
        Instruction::ImmediateArithmeticWord(i, function) => {
            let code = match function {
                ImmediateArithmeticWordFunction::Addiw => narrow::ADDIW,
                ImmediateArithmeticWordFunction::Slliw => narrow::SLLIW,
                ImmediateArithmeticWordFunction::Srliw => narrow::SRLIW,
                ImmediateArithmeticWordFunction::Sraiw => narrow::SRAIW,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        #[cfg(feature = "64-bit")]
        Instruction::RegisterArithmeticWord(r, function) => {
            let code = match function {
                RegisterArithmeticWordFunction::Addw => narrow::ADDW,
                RegisterArithmeticWordFunction::Subw => narrow::SUBW,
                RegisterArithmeticWordFunction::Sllw => narrow::SLLW,
                RegisterArithmeticWordFunction::Srlw => narrow::SRLW,
                RegisterArithmeticWordFunction::Sraw => narrow::SRAW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Mulw => narrow::MULW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divw => narrow::DIVW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divuw => narrow::DIVUW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remw => narrow::REMW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remuw => narrow::REMUW,
            };
            op(code, r.rd, r.rs1, r.rs2, 0)
        }
        #[cfg(feature = "a")]
        Instruction::Amo(r, function) => {
            // The immediate carries the access size in bytes.
            let size = 1u8 << r.funct3;
            if size != 4 && !(cfg!(feature = "64-bit") && size == 8) {
                return Err(Fault::BadAmoSize(size));
            }
            let code = match function {
                AmoFunction::Lr => narrow::LR,
                AmoFunction::Sc => narrow::SC,
                AmoFunction::Amoswap => narrow::AMOSWAP,
                AmoFunction::Amoadd => narrow::AMOADD,
                AmoFunction::Amoxor => narrow::AMOXOR,
                AmoFunction::Amoand => narrow::AMOAND,
                AmoFunction::Amoor => narrow::AMOOR,
                AmoFunction::Amomin => narrow::AMOMIN,
                AmoFunction::Amomax => narrow::AMOMAX,
                AmoFunction::Amominu => narrow::AMOMINU,
                AmoFunction::Amomaxu => narrow::AMOMAXU,
            };
            op(code, r.rd, r.rs1, r.rs2, size as XWord)
        }
    })
}
