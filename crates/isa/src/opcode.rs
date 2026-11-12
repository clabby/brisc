//! The instructions of the supported instruction sets, by mnemonic.

use crate::{arch::bytes_eq, Word};
use core::fmt;

/// The operands of an instruction, in the order assembly language writes them, and with that the
/// fields of the 32-bit encoding that hold them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// No operands.
    None,
    /// `rd, rs1, rs2`
    R,
    /// `rs1, rs2`: the fields of [`Format::R`] without a destination.
    Sources,
    /// `rd, rs1, imm`
    I,
    /// `rd, imm(rs1)`: the fields of [`Format::I`], written as a register and an address.
    Load,
    /// `rs2, imm(rs1)`
    S,
    /// `rs1, rs2, imm`, where the immediate is an offset from the instruction.
    B,
    /// `rd, imm`, where assembly language writes the upper 20 bits of the immediate.
    U,
    /// `rd, imm`, where the immediate is an offset from the instruction.
    J,
    /// `rd, (rs1)`: the fields of [`Format::R`] without a second source, and the ordering
    /// constraints in the immediate.
    Lr,
    /// `rd, rs2, (rs1)`: the fields of [`Format::R`], and the ordering constraints in the
    /// immediate.
    Amo,
    /// `rd, csr, rs1`: the fields of [`Format::I`], with the address of the CSR as the immediate.
    Csr,
    /// `rd, csr, uimm`: as [`Format::Csr`], with a 5-bit immediate in the place of `rs1`.
    CsrI,
}

/// Defines [`Opcode`] from one row per instruction: the documentation, the variant, the mnemonic,
/// the [`Format`], the bits of the 32-bit encoding that are not operands, and the crate features
/// the instruction requires.
macro_rules! opcodes {
    ($(
        $(#[$doc:meta])*
        $name:ident = $mnemonic:literal, $format:ident, $bits:literal
            $(, requires($($feature:literal),+))?;
    )*) => {
        /// An instruction of the base ISA or of one of the extensions this crate knows, without
        /// its operands.
        ///
        /// Every instruction is listed whatever features the crate is built with.
        /// [`Opcode::is_supported`] tells whether the enabled features include an instruction, and
        /// decoding only produces the ones they do.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum Opcode {
            $($(#[$doc])* $name,)*
        }

        impl Opcode {
            /// Every opcode, in the order of its discriminant.
            pub const ALL: [Self; [$($mnemonic),*].len()] = [$(Self::$name),*];

            /// Returns the mnemonic that assembly language uses for the instruction.
            pub const fn mnemonic(self) -> &'static str {
                match self {
                    $(Self::$name => $mnemonic,)*
                }
            }

            /// Returns the operands the instruction has.
            pub const fn format(self) -> Format {
                match self {
                    $(Self::$name => Format::$format,)*
                }
            }

            /// Returns the bits that every 32-bit encoding of the instruction has in common: the
            /// opcode and function fields, with the fields of the operands zero.
            pub const fn fixed_bits(self) -> Word {
                match self {
                    $(Self::$name => $bits,)*
                }
            }

            /// Returns whether the instruction belongs to the instruction set selected by the
            /// crate features.
            pub const fn is_supported(self) -> bool {
                match self {
                    $(Self::$name => cfg!(all($($(feature = $feature),+)?)),)*
                }
            }
        }
    };
}

opcodes! {
    // RV32I
    /// Load upper immediate.
    Lui = "lui", U, 0x0000_0037;
    /// Add upper immediate to the program counter.
    Auipc = "auipc", U, 0x0000_0017;
    /// Jump and link.
    Jal = "jal", J, 0x0000_006f;
    /// Jump and link register.
    Jalr = "jalr", Load, 0x0000_0067;
    /// Branch if equal.
    Beq = "beq", B, 0x0000_0063;
    /// Branch if not equal.
    Bne = "bne", B, 0x0000_1063;
    /// Branch if less than, signed.
    Blt = "blt", B, 0x0000_4063;
    /// Branch if greater than or equal, signed.
    Bge = "bge", B, 0x0000_5063;
    /// Branch if less than, unsigned.
    Bltu = "bltu", B, 0x0000_6063;
    /// Branch if greater than or equal, unsigned.
    Bgeu = "bgeu", B, 0x0000_7063;
    /// Load a byte, sign-extended.
    Lb = "lb", Load, 0x0000_0003;
    /// Load a halfword, sign-extended.
    Lh = "lh", Load, 0x0000_1003;
    /// Load a word, sign-extended.
    Lw = "lw", Load, 0x0000_2003;
    /// Load a byte, zero-extended.
    Lbu = "lbu", Load, 0x0000_4003;
    /// Load a halfword, zero-extended.
    Lhu = "lhu", Load, 0x0000_5003;
    /// Store a byte.
    Sb = "sb", S, 0x0000_0023;
    /// Store a halfword.
    Sh = "sh", S, 0x0000_1023;
    /// Store a word.
    Sw = "sw", S, 0x0000_2023;
    /// Add an immediate.
    Addi = "addi", I, 0x0000_0013;
    /// Set if less than an immediate, signed.
    Slti = "slti", I, 0x0000_2013;
    /// Set if less than an immediate, unsigned.
    Sltiu = "sltiu", I, 0x0000_3013;
    /// Exclusive or with an immediate.
    Xori = "xori", I, 0x0000_4013;
    /// Or with an immediate.
    Ori = "ori", I, 0x0000_6013;
    /// And with an immediate.
    Andi = "andi", I, 0x0000_7013;
    /// Shift left by an immediate.
    Slli = "slli", I, 0x0000_1013;
    /// Shift right by an immediate, filling with zeros.
    Srli = "srli", I, 0x0000_5013;
    /// Shift right by an immediate, filling with the sign.
    Srai = "srai", I, 0x4000_5013;
    /// Add.
    Add = "add", R, 0x0000_0033;
    /// Subtract.
    Sub = "sub", R, 0x4000_0033;
    /// Shift left.
    Sll = "sll", R, 0x0000_1033;
    /// Set if less than, signed.
    Slt = "slt", R, 0x0000_2033;
    /// Set if less than, unsigned.
    Sltu = "sltu", R, 0x0000_3033;
    /// Exclusive or.
    Xor = "xor", R, 0x0000_4033;
    /// Shift right, filling with zeros.
    Srl = "srl", R, 0x0000_5033;
    /// Shift right, filling with the sign.
    Sra = "sra", R, 0x4000_5033;
    /// Or.
    Or = "or", R, 0x0000_6033;
    /// And.
    And = "and", R, 0x0000_7033;
    /// Order memory and device accesses as seen by other harts and devices.
    Fence = "fence", I, 0x0000_000f;
    /// Call the execution environment.
    Ecall = "ecall", None, 0x0000_0073;
    /// Return control to a debugger.
    Ebreak = "ebreak", None, 0x0010_0073;

    // RV64I
    /// Load a word, zero-extended.
    Lwu = "lwu", Load, 0x0000_6003, requires("64-bit");
    /// Load a doubleword.
    Ld = "ld", Load, 0x0000_3003, requires("64-bit");
    /// Store a doubleword.
    Sd = "sd", S, 0x0000_3023, requires("64-bit");
    /// Add an immediate to a word.
    Addiw = "addiw", I, 0x0000_001b, requires("64-bit");
    /// Shift a word left by an immediate.
    Slliw = "slliw", I, 0x0000_101b, requires("64-bit");
    /// Shift a word right by an immediate, filling with zeros.
    Srliw = "srliw", I, 0x0000_501b, requires("64-bit");
    /// Shift a word right by an immediate, filling with the sign.
    Sraiw = "sraiw", I, 0x4000_501b, requires("64-bit");
    /// Add words.
    Addw = "addw", R, 0x0000_003b, requires("64-bit");
    /// Subtract words.
    Subw = "subw", R, 0x4000_003b, requires("64-bit");
    /// Shift a word left.
    Sllw = "sllw", R, 0x0000_103b, requires("64-bit");
    /// Shift a word right, filling with zeros.
    Srlw = "srlw", R, 0x0000_503b, requires("64-bit");
    /// Shift a word right, filling with the sign.
    Sraw = "sraw", R, 0x4000_503b, requires("64-bit");

    // Zifencei
    /// Make stores to instruction memory visible to instruction fetches.
    FenceI = "fence.i", I, 0x0000_100f;

    // Zicsr
    /// Swap a register with a control and status register.
    Csrrw = "csrrw", Csr, 0x0000_1073;
    /// Read a control and status register and set the bits of a register in it.
    Csrrs = "csrrs", Csr, 0x0000_2073;
    /// Read a control and status register and clear the bits of a register in it.
    Csrrc = "csrrc", Csr, 0x0000_3073;
    /// Read a control and status register and write an immediate to it.
    Csrrwi = "csrrwi", CsrI, 0x0000_5073;
    /// Read a control and status register and set the bits of an immediate in it.
    Csrrsi = "csrrsi", CsrI, 0x0000_6073;
    /// Read a control and status register and clear the bits of an immediate in it.
    Csrrci = "csrrci", CsrI, 0x0000_7073;

    // Privileged architecture
    /// Return from a trap taken in supervisor mode.
    Sret = "sret", None, 0x1020_0073;
    /// Return from a trap taken in machine mode.
    Mret = "mret", None, 0x3020_0073;
    /// Wait for an interrupt.
    Wfi = "wfi", None, 0x1050_0073;
    /// Make changes to the page tables visible to address translation.
    SfenceVma = "sfence.vma", Sources, 0x1200_0073;

    // M
    /// Multiply, low half of the product.
    Mul = "mul", R, 0x0200_0033, requires("m");
    /// Multiply signed by signed, high half of the product.
    Mulh = "mulh", R, 0x0200_1033, requires("m");
    /// Multiply signed by unsigned, high half of the product.
    Mulhsu = "mulhsu", R, 0x0200_2033, requires("m");
    /// Multiply unsigned by unsigned, high half of the product.
    Mulhu = "mulhu", R, 0x0200_3033, requires("m");
    /// Divide, signed.
    Div = "div", R, 0x0200_4033, requires("m");
    /// Divide, unsigned.
    Divu = "divu", R, 0x0200_5033, requires("m");
    /// Remainder, signed.
    Rem = "rem", R, 0x0200_6033, requires("m");
    /// Remainder, unsigned.
    Remu = "remu", R, 0x0200_7033, requires("m");

    // RV64M
    /// Multiply words.
    Mulw = "mulw", R, 0x0200_003b, requires("64-bit", "m");
    /// Divide words, signed.
    Divw = "divw", R, 0x0200_403b, requires("64-bit", "m");
    /// Divide words, unsigned.
    Divuw = "divuw", R, 0x0200_503b, requires("64-bit", "m");
    /// Remainder of words, signed.
    Remw = "remw", R, 0x0200_603b, requires("64-bit", "m");
    /// Remainder of words, unsigned.
    Remuw = "remuw", R, 0x0200_703b, requires("64-bit", "m");

    // A
    /// Load a word and reserve its address.
    LrW = "lr.w", Lr, 0x1000_202f, requires("a");
    /// Store a word if the reservation on its address still holds.
    ScW = "sc.w", Amo, 0x1800_202f, requires("a");
    /// Atomically swap a word in memory with a register.
    AmoswapW = "amoswap.w", Amo, 0x0800_202f, requires("a");
    /// Atomically add a register to a word in memory.
    AmoaddW = "amoadd.w", Amo, 0x0000_202f, requires("a");
    /// Atomically exclusive-or a register into a word in memory.
    AmoxorW = "amoxor.w", Amo, 0x2000_202f, requires("a");
    /// Atomically and a register into a word in memory.
    AmoandW = "amoand.w", Amo, 0x6000_202f, requires("a");
    /// Atomically or a register into a word in memory.
    AmoorW = "amoor.w", Amo, 0x4000_202f, requires("a");
    /// Atomically replace a word in memory with the signed minimum of it and a register.
    AmominW = "amomin.w", Amo, 0x8000_202f, requires("a");
    /// Atomically replace a word in memory with the signed maximum of it and a register.
    AmomaxW = "amomax.w", Amo, 0xa000_202f, requires("a");
    /// Atomically replace a word in memory with the unsigned minimum of it and a register.
    AmominuW = "amominu.w", Amo, 0xc000_202f, requires("a");
    /// Atomically replace a word in memory with the unsigned maximum of it and a register.
    AmomaxuW = "amomaxu.w", Amo, 0xe000_202f, requires("a");

    // RV64A
    /// Load a doubleword and reserve its address.
    LrD = "lr.d", Lr, 0x1000_302f, requires("64-bit", "a");
    /// Store a doubleword if the reservation on its address still holds.
    ScD = "sc.d", Amo, 0x1800_302f, requires("64-bit", "a");
    /// Atomically swap a doubleword in memory with a register.
    AmoswapD = "amoswap.d", Amo, 0x0800_302f, requires("64-bit", "a");
    /// Atomically add a register to a doubleword in memory.
    AmoaddD = "amoadd.d", Amo, 0x0000_302f, requires("64-bit", "a");
    /// Atomically exclusive-or a register into a doubleword in memory.
    AmoxorD = "amoxor.d", Amo, 0x2000_302f, requires("64-bit", "a");
    /// Atomically and a register into a doubleword in memory.
    AmoandD = "amoand.d", Amo, 0x6000_302f, requires("64-bit", "a");
    /// Atomically or a register into a doubleword in memory.
    AmoorD = "amoor.d", Amo, 0x4000_302f, requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the signed minimum of it and a register.
    AmominD = "amomin.d", Amo, 0x8000_302f, requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the signed maximum of it and a register.
    AmomaxD = "amomax.d", Amo, 0xa000_302f, requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the unsigned minimum of it and a register.
    AmominuD = "amominu.d", Amo, 0xc000_302f, requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the unsigned maximum of it and a register.
    AmomaxuD = "amomaxu.d", Amo, 0xe000_302f, requires("64-bit", "a");
}

impl Opcode {
    /// Returns the opcode with this mnemonic.
    pub const fn from_mnemonic(mnemonic: &str) -> Option<Self> {
        let mut i = 0;
        while i < Self::ALL.len() {
            if bytes_eq(Self::ALL[i].mnemonic().as_bytes(), mnemonic.as_bytes()) {
                return Some(Self::ALL[i]);
            }
            i += 1;
        }
        None
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn mnemonics_name_their_opcode() {
        for (discriminant, opcode) in Opcode::ALL.into_iter().enumerate() {
            assert_eq!(opcode as usize, discriminant);
            assert_eq!(Opcode::from_mnemonic(opcode.mnemonic()), Some(opcode));
        }
        assert_eq!(Opcode::from_mnemonic("addiu"), None);
    }
}
