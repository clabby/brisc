//! Checks [Instruction::decode] against the decoder it replaces.
//!
//! The two must agree on every word, except where the old decoder accepted a word that is not an
//! instruction, rejected a hint, or did not tell instructions apart. [known_difference] lists
//! those words.

use super::{
    BranchFunction, EnvironmentFunction, ImmediateArithmeticFunction, Instruction as Legacy,
    LoadFunction, RegisterArithmeticFunction, StoreFunction,
};
use crate::{bits, is_compressed, Instruction, Opcode, Word, X_LEN};

#[cfg(feature = "64-bit")]
use super::{ImmediateArithmeticWordFunction, RegisterArithmeticWordFunction};

#[cfg(feature = "a")]
use super::AmoFunction;

/// Returns the instruction that `legacy`, decoded from `word`, stands for, or `None` if it stands
/// for more than one.
fn translate(legacy: Legacy, word: Word) -> Option<Instruction> {
    use Opcode::*;

    let instruction = |opcode, rd, rs1, rs2, imm: crate::XWord| Instruction {
        opcode,
        rd,
        rs1,
        rs2,
        imm: imm as i32,
        length: if is_compressed(word) { 2 } else { 4 },
    };

    Some(match legacy {
        Legacy::MemoryLoad(i, function) => {
            let opcode = match function {
                LoadFunction::Lb => Lb,
                LoadFunction::Lh => Lh,
                LoadFunction::Lw => Lw,
                LoadFunction::Lbu => Lbu,
                LoadFunction::Lhu => Lhu,
                #[cfg(feature = "64-bit")]
                LoadFunction::Lwu => Lwu,
                #[cfg(feature = "64-bit")]
                LoadFunction::Ld => Ld,
            };
            instruction(opcode, i.rd, i.rs1, 0, i.imm)
        }
        Legacy::MemoryStore(s, function) => {
            let opcode = match function {
                StoreFunction::Sb => Sb,
                StoreFunction::Sh => Sh,
                StoreFunction::Sw => Sw,
                #[cfg(feature = "64-bit")]
                StoreFunction::Sd => Sd,
            };
            instruction(opcode, 0, s.rs1, s.rs2, s.imm)
        }
        Legacy::Branch(b, function) => {
            let opcode = match function {
                BranchFunction::Beq => Beq,
                BranchFunction::Bne => Bne,
                BranchFunction::Blt => Blt,
                BranchFunction::Bge => Bge,
                BranchFunction::Bltu => Bltu,
                BranchFunction::Bgeu => Bgeu,
            };
            instruction(opcode, 0, b.rs1, b.rs2, b.imm)
        }
        Legacy::ImmediateArithmetic(i, function) => {
            let shamt = i.imm & (X_LEN as crate::XWord - 1);
            let (opcode, imm) = match function {
                ImmediateArithmeticFunction::Addi => (Addi, i.imm),
                ImmediateArithmeticFunction::Xori => (Xori, i.imm),
                ImmediateArithmeticFunction::Ori => (Ori, i.imm),
                ImmediateArithmeticFunction::Andi => (Andi, i.imm),
                ImmediateArithmeticFunction::Slti => (Slti, i.imm),
                ImmediateArithmeticFunction::Sltiu => (Sltiu, i.imm),
                ImmediateArithmeticFunction::Slli => (Slli, shamt),
                ImmediateArithmeticFunction::Srli => (Srli, shamt),
                ImmediateArithmeticFunction::Srai => (Srai, shamt),
            };
            instruction(opcode, i.rd, i.rs1, 0, imm)
        }
        Legacy::RegisterArithmetic(r, function) => {
            let opcode = match function {
                RegisterArithmeticFunction::Add => Add,
                RegisterArithmeticFunction::Sub => Sub,
                RegisterArithmeticFunction::Xor => Xor,
                RegisterArithmeticFunction::Or => Or,
                RegisterArithmeticFunction::And => And,
                RegisterArithmeticFunction::Sll => Sll,
                RegisterArithmeticFunction::Srl => Srl,
                RegisterArithmeticFunction::Sra => Sra,
                RegisterArithmeticFunction::Slt => Slt,
                RegisterArithmeticFunction::Sltu => Sltu,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mul => Mul,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulh => Mulh,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhsu => Mulhsu,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhu => Mulhu,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Div => Div,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Divu => Divu,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Rem => Rem,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Remu => Remu,
            };
            instruction(opcode, r.rd, r.rs1, r.rs2, 0)
        }
        Legacy::Lui(u) => instruction(Lui, u.rd, 0, 0, u.imm),
        Legacy::Auipc(u) => instruction(Auipc, u.rd, 0, 0, u.imm),
        Legacy::Jal(j) => instruction(Jal, j.rd, 0, 0, j.imm),
        Legacy::Jalr(i) => instruction(Jalr, i.rd, i.rs1, 0, i.imm),
        Legacy::Environment(_, EnvironmentFunction::Ecall) => instruction(Ecall, 0, 0, 0, 0),
        // Stands for every other instruction with the SYSTEM opcode, except when it is the
        // expansion of a C.EBREAK.
        Legacy::Environment(_, EnvironmentFunction::Ebreak) if is_compressed(word) => {
            instruction(Ebreak, 0, 0, 0, 0)
        }
        Legacy::Environment(_, EnvironmentFunction::Ebreak) => return None,
        // Stands for `fence` and `fence.i` with any operands.
        Legacy::Fence => return None,
        #[cfg(feature = "64-bit")]
        Legacy::ImmediateArithmeticWord(i, function) => {
            let (opcode, imm) = match function {
                ImmediateArithmeticWordFunction::Addiw => (Addiw, i.imm),
                ImmediateArithmeticWordFunction::Slliw => (Slliw, i.imm & 31),
                ImmediateArithmeticWordFunction::Srliw => (Srliw, i.imm & 31),
                ImmediateArithmeticWordFunction::Sraiw => (Sraiw, i.imm & 31),
            };
            instruction(opcode, i.rd, i.rs1, 0, imm)
        }
        #[cfg(feature = "64-bit")]
        Legacy::RegisterArithmeticWord(r, function) => {
            let opcode = match function {
                RegisterArithmeticWordFunction::Addw => Addw,
                RegisterArithmeticWordFunction::Subw => Subw,
                RegisterArithmeticWordFunction::Sllw => Sllw,
                RegisterArithmeticWordFunction::Srlw => Srlw,
                RegisterArithmeticWordFunction::Sraw => Sraw,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Mulw => Mulw,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divw => Divw,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divuw => Divuw,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remw => Remw,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remuw => Remuw,
            };
            instruction(opcode, r.rd, r.rs1, r.rs2, 0)
        }
        #[cfg(feature = "a")]
        Legacy::Amo(r, function) => {
            let opcode = match (function, r.funct3) {
                (AmoFunction::Lr, 2) => LrW,
                (AmoFunction::Sc, 2) => ScW,
                (AmoFunction::Amoswap, 2) => AmoswapW,
                (AmoFunction::Amoadd, 2) => AmoaddW,
                (AmoFunction::Amoxor, 2) => AmoxorW,
                (AmoFunction::Amoand, 2) => AmoandW,
                (AmoFunction::Amoor, 2) => AmoorW,
                (AmoFunction::Amomin, 2) => AmominW,
                (AmoFunction::Amomax, 2) => AmomaxW,
                (AmoFunction::Amominu, 2) => AmominuW,
                (AmoFunction::Amomaxu, 2) => AmomaxuW,
                (AmoFunction::Lr, 3) => LrD,
                (AmoFunction::Sc, 3) => ScD,
                (AmoFunction::Amoswap, 3) => AmoswapD,
                (AmoFunction::Amoadd, 3) => AmoaddD,
                (AmoFunction::Amoxor, 3) => AmoxorD,
                (AmoFunction::Amoand, 3) => AmoandD,
                (AmoFunction::Amoor, 3) => AmoorD,
                (AmoFunction::Amomin, 3) => AmominD,
                (AmoFunction::Amomax, 3) => AmomaxD,
                (AmoFunction::Amominu, 3) => AmominuD,
                (AmoFunction::Amomaxu, 3) => AmomaxuD,
                // An access size that no instruction has: see [known_difference].
                _ => return None,
            };
            instruction(opcode, r.rd, r.rs1, r.rs2, (r.funct7 & 0b11) as crate::XWord)
        }
    })
}

/// Returns whether the decoders are known to disagree on `word`, and checks that they disagree
/// in the known way.
fn known_difference(
    word: Word,
    legacy: Option<Option<Instruction>>,
    new: Option<Instruction>,
) -> bool {
    let rd = bits!(u8, word, 7..12);
    let funct3 = bits!(u8, word, 12..15);
    let rejected = |expected: bool| {
        assert!(legacy.is_some() && new.is_none(), "{word:#010x}: {legacy:?} {new:?}");
        expected
    };
    let hint = |expected: bool| {
        assert!(legacy.is_none() && new.is_some_and(|new| new.rd == 0), "{word:#010x}: {new:?}");
        expected
    };

    if is_compressed(word) {
        let bit_12 = bits!(u8, word, 12..13);
        let imm = bits!(u8, word, 2..7);
        let wide_shift = cfg!(not(feature = "64-bit")) && bit_12 != 0;
        return match (word & 0b11, bits!(u8, word, 13..16)) {
            // C.ADDI16SP and C.LUI with a zero immediate were accepted, C.LUI to x0 was not.
            (0b01, 0b011) if bit_12 == 0 && imm == 0 => rejected(rd != 0),
            (0b01, 0b011) => hint(rd == 0),
            (0b01, 0b100) => match bits!(u8, word, 10..12) {
                // Shift amounts of 32 and more were accepted on a 32-bit architecture.
                0b00 | 0b01 => rejected(wide_shift),
                // The reserved encodings next to C.SUBW and C.ADDW decoded as C.OR and C.AND.
                0b11 => rejected(bit_12 != 0 && bits!(u8, word, 5..7) >= 0b10),
                _ => false,
            },
            // C.SLLI to x0 was rejected, and shift amounts of 32 and more were accepted on a
            // 32-bit architecture.
            (0b10, 0b000) if wide_shift => rejected(rd != 0),
            (0b10, 0b000) => hint(rd == 0),
            // C.JR x0 was accepted, C.ADD to x0 was not.
            (0b10, 0b100) if bit_12 == 0 => rejected(rd == 0 && imm == 0),
            (0b10, 0b100) => hint(rd == 0 && imm != 0),
            _ => false,
        };
    }

    match word & 0x7f {
        // Every word with the SYSTEM opcode decoded as `ecall` or `ebreak`, and every word with
        // the MISC-MEM opcode as `fence`.
        0b111_0011 | 0b000_1111 => {
            assert!(legacy.is_some(), "{word:#010x}");
            if new.is_some_and(|new| new.opcode == Opcode::Ecall) {
                assert_eq!(legacy, Some(new), "{word:#010x}");
            }
            true
        }
        // The funct3 of `jalr` was not checked.
        0b110_0111 => rejected(funct3 != 0),
        // The bits above the shift amount of `slli` were not checked.
        0b001_0011 => rejected(funct3 == 1 && word >> 20 >= X_LEN as Word),
        // The access size and the unused source register of `lr` were not checked.
        0b010_1111 if cfg!(feature = "a") => {
            let sized = funct3 == 2 || (cfg!(feature = "64-bit") && funct3 == 3);
            let lr = word >> 27 == 0b00010;
            rejected(!sized || (lr && bits!(u8, word, 20..25) != 0))
        }
        _ => false,
    }
}

fn check(word: Word) {
    let new = Instruction::decode(word).ok();
    let legacy = Legacy::try_from(word).ok().map(|legacy| translate(legacy, word));
    if legacy != new.map(Some) {
        assert!(
            known_difference(word, legacy, new),
            "{word:#010x}: legacy {legacy:?}, new {new:?}"
        );
    }
}

#[test]
fn every_compressed_instruction() {
    for word in 0..=u16::MAX {
        if is_compressed(word as Word) {
            check(word as Word);
            check(word as Word | 0xffff_0000);
        }
    }
}

/// Checks one word in 4099, so that the test is quick in an unoptimized build.
#[test]
fn sampled_words() {
    for word in (0..=Word::MAX).step_by(4099) {
        check(word | 0b11);
    }
}

#[test]
#[ignore = "takes a minute; run it in release mode"]
fn every_word() {
    let threads = std::thread::available_parallelism().map_or(1, usize::from) as Word;
    std::thread::scope(|scope| {
        for thread in 0..threads {
            scope.spawn(move || {
                for word in (thread..=Word::MAX >> 2).step_by(threads as usize) {
                    check(word << 2 | 0b11);
                }
            });
        }
    });
}
