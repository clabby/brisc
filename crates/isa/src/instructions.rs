//! RISC-V instructions and their decoding.

use crate::{Format, InstructionDecodeError, Opcode, Word, REGISTER_NAMES, X_LEN};
use core::fmt;

mod b_type;
pub use b_type::BType;

mod i_type;
pub use i_type::IType;

mod j_type;
pub use j_type::JType;

mod r_type;
pub use r_type::RType;

mod s_type;
pub use s_type::SType;

mod u_type;
pub use u_type::UType;

#[cfg(feature = "c")]
mod rvc;
#[cfg(feature = "c")]
pub use rvc::*;

/// Returns `true` if the instruction that starts with `parcel` is 16 bits long, which is the case
/// unless the two least significant bits are both set.
///
/// Only the low 16 bits of `parcel` are looked at, so it can be the first half of an instruction
/// or a word with the instruction in its low bits.
pub const fn is_compressed(parcel: Word) -> bool {
    parcel & 0b11 != 0b11
}

/// A decoded instruction: an [`Opcode`] with its operands.
///
/// A 16-bit instruction of the C extension decodes to the same opcode and operands as the 32-bit
/// instruction it abbreviates; only [`Instruction::length`] tells the two apart.
///
/// An operand that an instruction does not have is zero, so two instructions are equal exactly
/// when they have the same encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instruction {
    /// The instruction.
    pub opcode: Opcode,
    /// The destination register.
    pub rd: u8,
    /// The first source register.
    ///
    /// For [`Opcode::Csrrwi`], [`Opcode::Csrrsi`] and [`Opcode::Csrrci`] this is not a register
    /// but the 5-bit immediate that is written to the CSR.
    pub rs1: u8,
    /// The second source register.
    pub rs2: u8,
    /// The immediate, sign-extended. Every immediate of the supported instruction sets fits in
    /// 32 bits.
    ///
    /// - Loads, stores, branches, [`Opcode::Jal`] and [`Opcode::Jalr`]: the offset in bytes.
    /// - [`Opcode::Lui`] and [`Opcode::Auipc`]: the value the instruction loads or adds, a
    ///   multiple of 4096.
    /// - Shifts by an immediate: the shift amount.
    /// - CSR instructions: the address of the CSR, from 0 to 4095.
    /// - Instructions of the A extension: the ordering constraints, [`Instruction::AQ`] and
    ///   [`Instruction::RL`].
    /// - [`Opcode::Fence`] and [`Opcode::FenceI`]: the 12 bits above the `rs1` field, which hold
    ///   the predecessor and successor sets of a `fence`.
    pub imm: i32,
    /// The length of the instruction in bytes: 2 or 4.
    pub length: u8,
}

impl Instruction {
    /// The bit of [`Instruction::imm`] that is the `aq` bit of an instruction of the A extension.
    pub const AQ: i32 = 0b10;

    /// The bit of [`Instruction::imm`] that is the `rl` bit of an instruction of the A extension.
    pub const RL: i32 = 0b01;

    /// Decodes the instruction in the low bits of `word`.
    ///
    /// If the instruction is 16 bits long, the upper half of `word` is ignored.
    ///
    /// Decoding is exact: a word is an instruction only if the instruction sets selected by the
    /// crate features define it. Encodings that are reserved, or that belong to an extension
    /// that is not enabled, are errors. Hints, which are encodings of an instruction that only
    /// writes `x0`, decode as that instruction.
    pub const fn decode(word: Word) -> Result<Self, InstructionDecodeError> {
        use Opcode::*;

        #[cfg(feature = "c")]
        if is_compressed(word) {
            return match CompressedInstruction::decode(word as crate::HalfWord) {
                Ok(compressed) => Ok(compressed.expand()),
                Err(error) => Err(error),
            };
        }

        let major = (word & 0x7f) as u8;
        Ok(match major {
            // LOAD
            0b000_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Lb,
                    0b001 => Lh,
                    0b010 => Lw,
                    0b100 => Lbu,
                    0b101 => Lhu,
                    #[cfg(feature = "64-bit")]
                    0b110 => Lwu,
                    #[cfg(feature = "64-bit")]
                    0b011 => Ld,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                Self::from_i(opcode, i)
            }
            // MISC-MEM
            0b000_1111 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Fence,
                    0b001 => FenceI,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                Self::from_i(opcode, i)
            }
            // OP-IMM
            0b001_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Addi,
                    0b010 => Slti,
                    0b011 => Sltiu,
                    0b100 => Xori,
                    0b110 => Ori,
                    0b111 => Andi,
                    _ => return Self::decode_shift(i, X_LEN as i32, Slli, Srli, Srai),
                };
                Self::from_i(opcode, i)
            }
            0b001_0111 => Self::from_u(Auipc, UType::decode(word)),
            // OP-IMM-32
            #[cfg(feature = "64-bit")]
            0b001_1011 => {
                let i = IType::decode(word);
                match i.funct3 {
                    0b000 => Self::from_i(Addiw, i),
                    _ => return Self::decode_shift(i, 32, Slliw, Srliw, Sraiw),
                }
            }
            // STORE
            0b010_0011 => {
                let s = SType::decode(word);
                let opcode = match s.funct3 {
                    0b000 => Sb,
                    0b001 => Sh,
                    0b010 => Sw,
                    #[cfg(feature = "64-bit")]
                    0b011 => Sd,
                    _ => return Err(invalid_function(s.funct3, 0)),
                };
                Self { opcode, rd: 0, rs1: s.rs1, rs2: s.rs2, imm: s.imm, length: 4 }
            }
            // AMO
            #[cfg(feature = "a")]
            0b010_1111 => {
                let r = RType::decode(word);
                // The function is in the upper five bits of funct7, above the ordering bits.
                let opcode = match (r.funct3, r.funct7 >> 2) {
                    (0b010, 0b00010) => LrW,
                    (0b010, 0b00011) => ScW,
                    (0b010, 0b00001) => AmoswapW,
                    (0b010, 0b00000) => AmoaddW,
                    (0b010, 0b00100) => AmoxorW,
                    (0b010, 0b01100) => AmoandW,
                    (0b010, 0b01000) => AmoorW,
                    (0b010, 0b10000) => AmominW,
                    (0b010, 0b10100) => AmomaxW,
                    (0b010, 0b11000) => AmominuW,
                    (0b010, 0b11100) => AmomaxuW,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00010) => LrD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00011) => ScD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00001) => AmoswapD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00000) => AmoaddD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00100) => AmoxorD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b01100) => AmoandD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b01000) => AmoorD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b10000) => AmominD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b10100) => AmomaxD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b11000) => AmominuD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b11100) => AmomaxuD,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                // A load-reserved has no second source register.
                if matches!(opcode, LrW | LrD) && r.rs2 != 0 {
                    return Err(InstructionDecodeError::Reserved);
                }
                Self { imm: (r.funct7 & 0b11) as i32, ..Self::from_r(opcode, r) }
            }
            // OP
            0b011_0011 => {
                let r = RType::decode(word);
                let opcode = match (r.funct7, r.funct3) {
                    (0x00, 0b000) => Add,
                    (0x20, 0b000) => Sub,
                    (0x00, 0b001) => Sll,
                    (0x00, 0b010) => Slt,
                    (0x00, 0b011) => Sltu,
                    (0x00, 0b100) => Xor,
                    (0x00, 0b101) => Srl,
                    (0x20, 0b101) => Sra,
                    (0x00, 0b110) => Or,
                    (0x00, 0b111) => And,
                    #[cfg(feature = "m")]
                    (0x01, 0b000) => Mul,
                    #[cfg(feature = "m")]
                    (0x01, 0b001) => Mulh,
                    #[cfg(feature = "m")]
                    (0x01, 0b010) => Mulhsu,
                    #[cfg(feature = "m")]
                    (0x01, 0b011) => Mulhu,
                    #[cfg(feature = "m")]
                    (0x01, 0b100) => Div,
                    #[cfg(feature = "m")]
                    (0x01, 0b101) => Divu,
                    #[cfg(feature = "m")]
                    (0x01, 0b110) => Rem,
                    #[cfg(feature = "m")]
                    (0x01, 0b111) => Remu,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                Self::from_r(opcode, r)
            }
            0b011_0111 => Self::from_u(Lui, UType::decode(word)),
            // OP-32
            #[cfg(feature = "64-bit")]
            0b011_1011 => {
                let r = RType::decode(word);
                let opcode = match (r.funct7, r.funct3) {
                    (0x00, 0b000) => Addw,
                    (0x20, 0b000) => Subw,
                    (0x00, 0b001) => Sllw,
                    (0x00, 0b101) => Srlw,
                    (0x20, 0b101) => Sraw,
                    #[cfg(feature = "m")]
                    (0x01, 0b000) => Mulw,
                    #[cfg(feature = "m")]
                    (0x01, 0b100) => Divw,
                    #[cfg(feature = "m")]
                    (0x01, 0b101) => Divuw,
                    #[cfg(feature = "m")]
                    (0x01, 0b110) => Remw,
                    #[cfg(feature = "m")]
                    (0x01, 0b111) => Remuw,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                Self::from_r(opcode, r)
            }
            // BRANCH
            0b110_0011 => {
                let b = BType::decode(word);
                let opcode = match b.funct3 {
                    0b000 => Beq,
                    0b001 => Bne,
                    0b100 => Blt,
                    0b101 => Bge,
                    0b110 => Bltu,
                    0b111 => Bgeu,
                    _ => return Err(invalid_function(b.funct3, 0)),
                };
                Self { opcode, rd: 0, rs1: b.rs1, rs2: b.rs2, imm: b.imm, length: 4 }
            }
            0b110_0111 => {
                let i = IType::decode(word);
                if i.funct3 != 0 {
                    return Err(invalid_function(i.funct3, 0));
                }
                Self::from_i(Jalr, i)
            }
            0b110_1111 => {
                let j = JType::decode(word);
                Self { opcode: Jal, rd: j.rd, rs1: 0, rs2: 0, imm: j.imm, length: 4 }
            }
            // SYSTEM
            0b111_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => return Self::decode_privileged(word),
                    0b001 => Csrrw,
                    0b010 => Csrrs,
                    0b011 => Csrrc,
                    0b101 => Csrrwi,
                    0b110 => Csrrsi,
                    0b111 => Csrrci,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                // The address of a CSR is not sign-extended.
                Self { imm: (word >> 20) as i32, ..Self::from_i(opcode, i) }
            }
            _ => return Err(InstructionDecodeError::InvalidOpcode(major)),
        })
    }

    /// Decodes a shift by an immediate of a value that is `width` bits wide.
    ///
    /// The low bits of the immediate are the shift amount, as many as it takes to index a bit of
    /// the value. The bits above them select between a logical and an arithmetic shift.
    const fn decode_shift(
        i: IType,
        width: i32,
        left: Opcode,
        right_logical: Opcode,
        right_arithmetic: Opcode,
    ) -> Result<Self, InstructionDecodeError> {
        let shamt = i.imm & (width - 1);
        let function = (i.imm & 0xfff) ^ shamt;
        let opcode = match (i.funct3, function) {
            (0b001, 0x000) => left,
            (0b101, 0x000) => right_logical,
            (0b101, 0x400) => right_arithmetic,
            _ => return Err(invalid_function(i.funct3, (function >> 5) as u8)),
        };
        Ok(Self { imm: shamt, ..Self::from_i(opcode, i) })
    }

    /// Decodes an instruction with the SYSTEM major opcode and a funct3 of zero. Of these only
    /// `sfence.vma` has operands.
    const fn decode_privileged(word: Word) -> Result<Self, InstructionDecodeError> {
        let opcode = match word {
            0x0000_0073 => Opcode::Ecall,
            0x0010_0073 => Opcode::Ebreak,
            0x1020_0073 => Opcode::Sret,
            0x3020_0073 => Opcode::Mret,
            0x1050_0073 => Opcode::Wfi,
            _ => {
                let r = RType::decode(word);
                if r.funct7 != 0b000_1001 || r.rd != 0 {
                    return Err(invalid_function(r.funct3, r.funct7));
                }
                return Ok(Self::from_r(Opcode::SfenceVma, r));
            }
        };
        Ok(Self { opcode, rd: 0, rs1: 0, rs2: 0, imm: 0, length: 4 })
    }

    /// Encodes the instruction in 32 bits, whatever its [length](Instruction::length).
    ///
    /// Returns `None` if the result would not decode to these operands: an operand does not fit
    /// its field or is not zero although the instruction does not have it, or the crate
    /// features do not include the instruction.
    pub const fn encode(&self) -> Option<Word> {
        let Self { opcode, rd, rs1, rs2, imm, .. } = *self;
        let operands = match opcode.format() {
            Format::None => 0,
            Format::R | Format::Sources => RType { rd, funct3: 0, rs1, rs2, funct7: 0 }.encode(),
            // The ordering constraints are the low bits of funct7.
            Format::Lr | Format::Amo => {
                RType { rd, funct3: 0, rs1, rs2, funct7: imm as u8 }.encode()
            }
            Format::I | Format::Load | Format::Csr | Format::CsrI => {
                IType { rd, funct3: 0, rs1, imm }.encode()
            }
            Format::S => SType { funct3: 0, rs1, rs2, imm }.encode(),
            Format::B => BType { funct3: 0, rs1, rs2, imm }.encode(),
            Format::U => UType { rd, imm }.encode(),
            Format::J => JType { rd, imm }.encode(),
        };
        let word = opcode.fixed_bits() | operands;

        // Decoding is exact, so it is also the check that every operand survived.
        match Self::decode(word) {
            Ok(decoded)
                if decoded.opcode as u8 == opcode as u8 &&
                    decoded.rd == rd &&
                    decoded.rs1 == rs1 &&
                    decoded.rs2 == rs2 &&
                    decoded.imm == imm =>
            {
                Some(word)
            }
            _ => None,
        }
    }

    const fn from_r(opcode: Opcode, r: RType) -> Self {
        Self { opcode, rd: r.rd, rs1: r.rs1, rs2: r.rs2, imm: 0, length: 4 }
    }

    const fn from_i(opcode: Opcode, i: IType) -> Self {
        Self { opcode, rd: i.rd, rs1: i.rs1, rs2: 0, imm: i.imm, length: 4 }
    }

    const fn from_u(opcode: Opcode, u: UType) -> Self {
        Self { opcode, rd: u.rd, rs1: 0, rs2: 0, imm: u.imm, length: 4 }
    }
}

impl TryFrom<Word> for Instruction {
    type Error = InstructionDecodeError;

    fn try_from(word: Word) -> Result<Self, Self::Error> {
        Self::decode(word)
    }
}

/// Writes the instruction in assembly language, in the form that [rv!](crate::rv) reads: the
/// mnemonic and the operands of its [`Format`], with registers by the names of the standard
/// calling convention and immediates in decimal.
impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { opcode, imm, .. } = *self;
        let [rd, rs1, rs2] = [self.rd, self.rs1, self.rs2]
            .map(|register| REGISTER_NAMES.get(register as usize).copied().unwrap_or("invalid"));
        let ordering = ["", ".rl", ".aq", ".aqrl"][(imm & (Self::AQ | Self::RL)) as usize];

        match opcode.format() {
            Format::None => write!(f, "{opcode}"),
            Format::R => write!(f, "{opcode} {rd}, {rs1}, {rs2}"),
            Format::Sources => write!(f, "{opcode} {rs1}, {rs2}"),
            Format::I => write!(f, "{opcode} {rd}, {rs1}, {imm}"),
            Format::Load => write!(f, "{opcode} {rd}, {imm}({rs1})"),
            Format::S => write!(f, "{opcode} {rs2}, {imm}({rs1})"),
            Format::B => write!(f, "{opcode} {rs1}, {rs2}, {imm}"),
            Format::U => write!(f, "{opcode} {rd}, {}", imm as u32 >> 12),
            Format::J => write!(f, "{opcode} {rd}, {imm}"),
            Format::Lr => write!(f, "{opcode}{ordering} {rd}, ({rs1})"),
            Format::Amo => write!(f, "{opcode}{ordering} {rd}, {rs2}, ({rs1})"),
            Format::Csr => write!(f, "{opcode} {rd}, {imm}, {rs1}"),
            Format::CsrI => write!(f, "{opcode} {rd}, {imm}, {}", self.rs1),
        }
    }
}

const fn invalid_function(q_a: u8, q_b: u8) -> InstructionDecodeError {
    InstructionDecodeError::InvalidFunction { q_a, q_b }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn all_zero_parcels_are_illegal() {
        assert!(Instruction::decode(0x0000).is_err());
        assert!(Instruction::decode(0x0000_0000).is_err());

        // A zero immediate is reserved for `c.addi4spn` whichever register it names.
        #[cfg(feature = "c")]
        assert!(Instruction::decode(0b000_00000000_001_00).is_err());
        #[cfg(feature = "c")]
        assert!(Instruction::decode(0b000_00000001_001_00).is_ok());
    }

    /// Together with the check inside [`Instruction::encode`], this makes decoding and encoding
    /// inverses of each other: no two words decode to the same instruction, and decoding
    /// keeps every bit of a word that is an instruction.
    #[test]
    fn encode_inverts_decode_on_every_word() {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        std::thread::scope(|scope| {
            for thread in 0..threads {
                scope.spawn(move || {
                    // Every word that is not a 16-bit instruction ends in two set bits.
                    for upper in (thread as Word..=Word::MAX >> 2).step_by(threads) {
                        let word = upper << 2 | 0b11;
                        if let Ok(instruction) = Instruction::decode(word) {
                            assert_eq!(instruction.length, 4);
                            assert_eq!(instruction.encode(), Some(word), "{word:#010x}");
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn exactly_the_supported_opcodes_encode() {
        for opcode in Opcode::ALL {
            let instruction = Instruction { opcode, rd: 0, rs1: 0, rs2: 0, imm: 0, length: 4 };
            let word = instruction.encode();
            assert_eq!(word.is_some(), opcode.is_supported(), "{opcode}");
            if let Some(word) = word {
                assert_eq!(Instruction::decode(word), Ok(instruction));
            }
        }
    }

    #[test]
    fn operands_that_do_not_fit_do_not_encode() {
        let addi =
            Instruction { opcode: Opcode::Addi, rd: 1, rs1: 2, rs2: 0, imm: 2047, length: 4 };
        assert!(addi.encode().is_some());
        assert_eq!(Instruction { imm: 2048, ..addi }.encode(), None);
        assert_eq!(Instruction { rd: 32, ..addi }.encode(), None);
        assert_eq!(Instruction { rs2: 1, ..addi }.encode(), None);

        let beq = Instruction { opcode: Opcode::Beq, rd: 0, rs1: 1, rs2: 2, imm: -4096, length: 4 };
        assert!(beq.encode().is_some());
        assert_eq!(Instruction { imm: 3, ..beq }.encode(), None);
        assert_eq!(Instruction { imm: 4096, ..beq }.encode(), None);
    }

    #[test]
    fn display_writes_assembly() {
        let text = |word| Instruction::decode(word).unwrap().to_string();
        assert_eq!(text(crate::rv!(addi a0, zero, -5)), "addi a0, zero, -5");
        assert_eq!(text(crate::rv!(sw s1, 12(sp))), "sw s1, 12(sp)");
        assert_eq!(text(crate::rv!(lui t0, 0xfffff)), "lui t0, 1048575");
        assert_eq!(text(crate::rv!(csrrwi x5, 0x340, 31)), "csrrwi t0, 832, 31");
        assert_eq!(text(crate::rv!(ecall)), "ecall");
        #[cfg(feature = "a")]
        assert_eq!(text(crate::rv!(sc.w.rl a0, a2, (a1))), "sc.w.rl a0, a2, (a1)");
    }

    mod llvm {
        //! Checks the decoder, the encoder and the assembly syntax against LLVM.
        //!
        //! The tests need `llvm-mc` and `llvm-objdump` with support for RISC-V. They are looked for
        //! in the directory named by the `LLVM_BIN` environment variable, then on the
        //! `PATH`, then where Homebrew and Debian install LLVM. Without them the tests pass
        //! without checking anything, unless `LLVM_BIN` is set, in which case they fail.

        use crate::{is_compressed, Format, Instruction, Opcode, Word};
        use std::{
            env, fs,
            io::Write,
            path::{Path, PathBuf},
            process::{Command, Stdio},
        };

        /// The LLVM tools.
        struct Llvm {
            mc: PathBuf,
            objdump: PathBuf,
        }

        impl Llvm {
            /// Finds the tools, or returns `None` if they are not installed.
            fn find() -> Option<Self> {
                let in_directory = |directory: &Path| {
                    let llvm = Self {
                        mc: directory.join("llvm-mc"),
                        objdump: directory.join("llvm-objdump"),
                    };
                    let version = Command::new(&llvm.mc).arg("--version").output().ok()?;
                    let supports_riscv =
                        String::from_utf8_lossy(&version.stdout).contains("riscv64");
                    (supports_riscv && llvm.objdump.exists()).then_some(llvm)
                };

                if let Some(directory) = env::var_os("LLVM_BIN") {
                    let llvm = in_directory(Path::new(&directory));
                    assert!(
                        llvm.is_some(),
                        "LLVM_BIN does not hold llvm-mc and llvm-objdump for RISC-V"
                    );
                    return llvm;
                }

                let mut directories = env::split_paths(&env::var_os("PATH").unwrap_or_default())
                    .chain(["/opt/homebrew/opt/llvm/bin".into(), "/usr/local/opt/llvm/bin".into()])
                    .collect::<Vec<_>>();
                // Debian installs each version in a directory of its own.
                let mut versioned = fs::read_dir("/usr/lib")
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("llvm-"))
                    .map(|entry| entry.path().join("bin"))
                    .collect::<Vec<_>>();
                versioned.sort();
                directories.extend(versioned.into_iter().rev());

                let llvm = directories.iter().find_map(|directory| in_directory(directory));
                if llvm.is_none() {
                    eprintln!("llvm-mc and llvm-objdump for RISC-V are not installed: nothing was checked");
                }
                llvm
            }

            /// Returns the LLVM names of the architecture and of the instruction sets that the
            /// crate features select.
            fn target() -> (String, String) {
                let architecture = if cfg!(feature = "64-bit") { "riscv64" } else { "riscv32" };
                let sign = |enabled| if enabled { '+' } else { '-' };
                let attributes = format!(
                    "{}m,{}a,{}c,+zicsr,+zifencei",
                    sign(cfg!(feature = "m")),
                    sign(cfg!(feature = "a")),
                    sign(cfg!(feature = "c")),
                );
                (architecture.into(), attributes)
            }

            /// Runs `llvm-mc` with `arguments` on the assembly `source` and returns what it prints.
            ///
            /// ## Panics
            ///
            /// Panics if the source does not assemble.
            fn mc(&self, source: &str, arguments: &[&str]) -> String {
                let (architecture, attributes) = Self::target();
                let mut child = Command::new(&self.mc)
                    .args([&format!("-triple={architecture}"), &format!("-mattr={attributes}")])
                    .args(arguments)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                // The source is written from another thread so that the pipes cannot fill up.
                let mut stdin = child.stdin.take().unwrap();
                let output = std::thread::scope(|scope| {
                    scope.spawn(move || stdin.write_all(source.as_bytes()).unwrap());
                    child.wait_with_output().unwrap()
                });
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                String::from_utf8(output.stdout).unwrap()
            }

            /// Assembles each line of `source` to a 32-bit instruction, also where the C extension
            /// has a shorter one.
            fn assemble(&self, source: &str) -> Vec<Word> {
                self.mc(&format!(".option norvc\n{source}"), &["-show-encoding"])
                    .lines()
                    .filter_map(|line| line.split_once("# encoding: [")?.1.strip_suffix(']'))
                    .map(|encoding| {
                        let bytes = encoding
                            .split(',')
                            .map(|byte| {
                                u8::from_str_radix(byte.trim_start_matches("0x"), 16).unwrap()
                            })
                            .collect::<Vec<_>>();
                        Word::from_le_bytes(bytes.try_into().expect("an instruction of four bytes"))
                    })
                    .collect()
            }

            /// Disassembles each of `words`, which hold an instruction of 16 or 32 bits in their
            /// low bits. Returns the mnemonic and the operands, or `None` for a word
            /// that is not an instruction.
            ///
            /// The instruction in `words[i]` is disassembled at address `4 * i`, and operands that
            /// are addresses are relative to that.
            fn disassemble(&self, name: &str, words: &[Word]) -> Vec<Option<(String, String)>> {
                // Every word gets a label, because the disassembler starts afresh at each one, and
                // a 16-bit instruction is followed by a `c.nop` to keep the
                // addresses regular.
                let mut source = String::from(".text\n");
                for (i, word) in words.iter().enumerate() {
                    if is_compressed(*word) {
                        source.push_str(&format!(
                            "r{i}: .insn 2, {:#x}\n.insn 2, 1\n",
                            word & 0xffff
                        ));
                    } else {
                        source.push_str(&format!("r{i}: .insn 4, {word:#x}\n"));
                    }
                }
                let object =
                    env::temp_dir().join(format!("brisc-isa-{}-{name}.o", std::process::id()));
                self.mc(&source, &["-filetype=obj", "-o", object.to_str().unwrap()]);

                let (_, attributes) = Self::target();
                let output = Command::new(&self.objdump)
                    .args(["-d", "--no-show-raw-insn", &format!("--mattr={attributes}")])
                    .arg(&object)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                fs::remove_file(&object).unwrap();

                // The line after the one with a label holds the instruction at the label.
                let listing = String::from_utf8(output.stdout).unwrap();
                let mut lines = listing.lines();
                let mut instructions = Vec::with_capacity(words.len());
                while let Some(line) = lines.next() {
                    if line.ends_with(&format!(" <r{}>:", instructions.len())) {
                        let mut fields = lines.next().unwrap().split('\t').skip(1);
                        let mnemonic = fields.next().unwrap();
                        let operands = fields.next().unwrap_or_default();
                        instructions.push(
                            (mnemonic != "<unknown>").then(|| (mnemonic.into(), operands.into())),
                        );
                    }
                }
                assert_eq!(instructions.len(), words.len());
                instructions
            }
        }

        /// A xorshift generator: the tests need arbitrary words, not good randomness.
        struct Random(u64);

        impl Random {
            const fn next(&mut self) -> Word {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                (self.0 >> 16) as Word
            }
        }

        /// Returns words with the fixed bits of every opcode and arbitrary bits in the fields of
        /// its operands. Most are instructions; some are not, because an operand is out of
        /// range.
        fn words_of_every_opcode(random: &mut Random) -> Vec<Word> {
            let mut words = Vec::new();
            for opcode in Opcode::ALL {
                let operands = match opcode.format() {
                    Format::None => 0,
                    Format::R | Format::Amo => 0x07ff_8f80,
                    Format::Sources => 0x01ff_8000,
                    Format::Lr => 0x060f_8f80,
                    Format::I |
                    Format::Load |
                    Format::Csr |
                    Format::CsrI |
                    Format::S |
                    Format::B => 0xffff_8f80,
                    Format::U | Format::J => 0xffff_ff80,
                };
                // Shift amounts are a small part of the immediate, so the bits above them are only
                // rarely set.
                let shift =
                    matches!(opcode.mnemonic().as_bytes(), [b's', b'l' | b'r', _, b'i', ..]);
                for _ in 0..200 {
                    let mut word = opcode.fixed_bits() | (random.next() & operands);
                    if shift && !random.next().is_multiple_of(8) {
                        word &= opcode.fixed_bits() | 0x03ff_ffff;
                    }
                    words.push(word);
                }
            }
            words
        }

        /// Returns whether LLVM and this crate are known to disagree on whether `word` is an
        /// instruction.
        const fn known_disagreement(word: Word, decoded: Option<Instruction>) -> bool {
            // The fields of a fence that have no meaning yet are to be ignored, which is what
            // decoding them as operands amounts to. LLVM only knows fences in which
            // they are zero.
            if let Some(instruction) = decoded {
                return matches!(instruction.opcode, Opcode::Fence | Opcode::FenceI);
            }
            // LLVM reads the all-zero halfword, which is defined to be illegal, as `c.unimp`, and
            // knows the return from debug mode.
            word & 0xffff == 0 || word == 0x7b20_0073
        }

        /// LLVM and [`Instruction::decode`] accept the same words as instructions.
        #[test]
        fn decoding_accepts_what_llvm_accepts() {
            let Some(llvm) = Llvm::find() else { return };
            let mut random = Random(0x9e37_79b9_7f4a_7c15);

            // Every 16-bit instruction if the C extension is enabled, every opcode with arbitrary
            // operands, each of those with one more bit flipped, and arbitrary words.
            let mut words = (0..=u16::MAX as Word)
                .filter(|word| cfg!(feature = "c") && is_compressed(*word))
                .collect::<Vec<_>>();
            let with_operands = words_of_every_opcode(&mut random);
            words.extend(with_operands.iter().map(|word| word ^ (4 << (random.next() % 30))));
            words.extend(with_operands);
            words.extend((0..20_000).map(|_| random.next() | 0b11));

            let disassembly = llvm.disassemble("accepts", &words);
            for (word, llvm) in words.into_iter().zip(disassembly) {
                let decoded = Instruction::decode(word).ok();
                if decoded.is_some() != llvm.is_some() {
                    assert!(
                        known_disagreement(word, decoded),
                        "{word:#010x}: {decoded:?}, llvm: {llvm:?}"
                    );
                }
            }
        }

        /// LLVM assembles the text that [`Instruction`] displays as to the word that
        /// [`Instruction::encode`] returns.
        #[test]
        fn encoding_agrees_with_the_llvm_assembler() {
            let Some(llvm) = Llvm::find() else { return };
            let mut random = Random(0x2545_f491_4f6c_dd1d);

            let instructions = words_of_every_opcode(&mut random)
                .into_iter()
                .filter_map(|word| Instruction::decode(word).ok())
                // The standard syntax of these two has other operands than their fields.
                .filter(|instruction| !matches!(instruction.opcode, Opcode::Fence | Opcode::FenceI))
                .collect::<Vec<_>>();
            for opcode in Opcode::ALL {
                let covered = instructions.iter().any(|instruction| instruction.opcode == opcode);
                let expected =
                    opcode.is_supported() && !matches!(opcode, Opcode::Fence | Opcode::FenceI);
                assert_eq!(covered, expected, "{opcode}");
            }

            let source = instructions
                .iter()
                .map(|instruction| format!("{instruction}\n"))
                .collect::<String>();
            let assembled = llvm.assemble(&source);
            assert_eq!(assembled.len(), instructions.len());
            for (instruction, assembled) in instructions.into_iter().zip(assembled) {
                assert_eq!(instruction.encode(), Some(assembled), "{instruction}");
            }
        }

        /// A 16-bit instruction decodes to the instruction that LLVM says it abbreviates.
        ///
        /// LLVM disassembles a 16-bit instruction as the 32-bit instruction it stands for.
        /// Assembling that text without the C extension gives a 32-bit word, which must
        /// decode to the same operands as the 16-bit instruction.
        #[test]
        #[cfg(feature = "c")]
        fn compressed_instructions_expand_as_llvm_expands_them() {
            let Some(llvm) = Llvm::find() else { return };

            let words = (0..=u16::MAX as Word)
                .filter(|word| is_compressed(*word) && Instruction::decode(*word).is_ok())
                .collect::<Vec<_>>();
            let disassembly = llvm.disassemble("expands", &words);
            let instructions = words.len();

            let mut compared = Vec::new();
            let mut source = String::new();
            for (i, (word, llvm)) in words.into_iter().zip(disassembly).enumerate() {
                let (mnemonic, operands) =
                    llvm.expect("checked by decoding_accepts_what_llvm_accepts");
                // LLVM does not expand hints, which only write `x0`.
                if mnemonic.starts_with("c.") {
                    continue;
                }
                // An operand that is an address is followed by the label it is relative to. The
                // assembler wants the offset from the instruction instead.
                let operands = match operands.split_once(" <") {
                    Some((operands, _)) => {
                        let (registers, address) =
                            operands.rsplit_once(' ').unwrap_or(("", operands));
                        let address =
                            u64::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
                        let offset = address.wrapping_sub(4 * i as u64);
                        // Addresses wrap around at the width of a register.
                        let offset = if cfg!(feature = "64-bit") {
                            offset as i64
                        } else {
                            offset as i32 as i64
                        };
                        format!("{registers} {offset}")
                    }
                    None => operands,
                };
                // LLVM writes `add rd, zero, rs2`, the expansion of `c.mv`, as `mv rd, rs2`, which
                // the assembler takes to mean `addi rd, rs2, 0`.
                match operands.split_once(", ") {
                    Some((rd, rs2)) if mnemonic == "mv" => {
                        source.push_str(&format!("add {rd}, zero, {rs2}\n"));
                    }
                    _ => source.push_str(&format!("{mnemonic} {operands}\n")),
                }
                compared.push(word);
            }
            // Hints are a small part of the encodings.
            assert!(compared.len() > instructions / 10 * 9, "{} of {instructions}", compared.len());

            let assembled = llvm.assemble(&source);
            assert_eq!(assembled.len(), compared.len());
            for (word, assembled) in compared.into_iter().zip(assembled) {
                let compressed = Instruction::decode(word).unwrap();
                let expansion = Instruction::decode(assembled);
                assert_eq!(compressed.length, 2);
                assert_eq!(expansion, Ok(Instruction { length: 4, ..compressed }), "{word:#06x}");
            }
        }
    }
}
