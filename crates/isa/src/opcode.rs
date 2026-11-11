//! The instructions of the supported instruction sets, by mnemonic.

use core::fmt;

/// Defines [Opcode] from one row per instruction: the documentation, the variant, the mnemonic
/// and the crate features the instruction requires.
macro_rules! opcodes {
    ($(
        $(#[$doc:meta])*
        $name:ident = $mnemonic:literal $(, requires($($feature:literal),+))?;
    )*) => {
        /// An instruction of the base ISA or of one of the extensions this crate knows, without
        /// its operands.
        ///
        /// Every instruction is listed whatever features the crate is built with.
        /// [Opcode::is_supported] tells whether the enabled features include an instruction, and
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
    Lui = "lui";
    /// Add upper immediate to the program counter.
    Auipc = "auipc";
    /// Jump and link.
    Jal = "jal";
    /// Jump and link register.
    Jalr = "jalr";
    /// Branch if equal.
    Beq = "beq";
    /// Branch if not equal.
    Bne = "bne";
    /// Branch if less than, signed.
    Blt = "blt";
    /// Branch if greater than or equal, signed.
    Bge = "bge";
    /// Branch if less than, unsigned.
    Bltu = "bltu";
    /// Branch if greater than or equal, unsigned.
    Bgeu = "bgeu";
    /// Load a byte, sign-extended.
    Lb = "lb";
    /// Load a halfword, sign-extended.
    Lh = "lh";
    /// Load a word, sign-extended.
    Lw = "lw";
    /// Load a byte, zero-extended.
    Lbu = "lbu";
    /// Load a halfword, zero-extended.
    Lhu = "lhu";
    /// Store a byte.
    Sb = "sb";
    /// Store a halfword.
    Sh = "sh";
    /// Store a word.
    Sw = "sw";
    /// Add an immediate.
    Addi = "addi";
    /// Set if less than an immediate, signed.
    Slti = "slti";
    /// Set if less than an immediate, unsigned.
    Sltiu = "sltiu";
    /// Exclusive or with an immediate.
    Xori = "xori";
    /// Or with an immediate.
    Ori = "ori";
    /// And with an immediate.
    Andi = "andi";
    /// Shift left by an immediate.
    Slli = "slli";
    /// Shift right by an immediate, filling with zeros.
    Srli = "srli";
    /// Shift right by an immediate, filling with the sign.
    Srai = "srai";
    /// Add.
    Add = "add";
    /// Subtract.
    Sub = "sub";
    /// Shift left.
    Sll = "sll";
    /// Set if less than, signed.
    Slt = "slt";
    /// Set if less than, unsigned.
    Sltu = "sltu";
    /// Exclusive or.
    Xor = "xor";
    /// Shift right, filling with zeros.
    Srl = "srl";
    /// Shift right, filling with the sign.
    Sra = "sra";
    /// Or.
    Or = "or";
    /// And.
    And = "and";
    /// Order memory and device accesses as seen by other harts and devices.
    Fence = "fence";
    /// Call the execution environment.
    Ecall = "ecall";
    /// Return control to a debugger.
    Ebreak = "ebreak";

    // RV64I
    /// Load a word, zero-extended.
    Lwu = "lwu", requires("64-bit");
    /// Load a doubleword.
    Ld = "ld", requires("64-bit");
    /// Store a doubleword.
    Sd = "sd", requires("64-bit");
    /// Add an immediate to a word.
    Addiw = "addiw", requires("64-bit");
    /// Shift a word left by an immediate.
    Slliw = "slliw", requires("64-bit");
    /// Shift a word right by an immediate, filling with zeros.
    Srliw = "srliw", requires("64-bit");
    /// Shift a word right by an immediate, filling with the sign.
    Sraiw = "sraiw", requires("64-bit");
    /// Add words.
    Addw = "addw", requires("64-bit");
    /// Subtract words.
    Subw = "subw", requires("64-bit");
    /// Shift a word left.
    Sllw = "sllw", requires("64-bit");
    /// Shift a word right, filling with zeros.
    Srlw = "srlw", requires("64-bit");
    /// Shift a word right, filling with the sign.
    Sraw = "sraw", requires("64-bit");

    // Zifencei
    /// Make stores to instruction memory visible to instruction fetches.
    FenceI = "fence.i";

    // Zicsr
    /// Swap a register with a control and status register.
    Csrrw = "csrrw";
    /// Read a control and status register and set the bits of a register in it.
    Csrrs = "csrrs";
    /// Read a control and status register and clear the bits of a register in it.
    Csrrc = "csrrc";
    /// Read a control and status register and write an immediate to it.
    Csrrwi = "csrrwi";
    /// Read a control and status register and set the bits of an immediate in it.
    Csrrsi = "csrrsi";
    /// Read a control and status register and clear the bits of an immediate in it.
    Csrrci = "csrrci";

    // Privileged architecture
    /// Return from a trap taken in supervisor mode.
    Sret = "sret";
    /// Return from a trap taken in machine mode.
    Mret = "mret";
    /// Wait for an interrupt.
    Wfi = "wfi";
    /// Make changes to the page tables visible to address translation.
    SfenceVma = "sfence.vma";

    // M
    /// Multiply, low half of the product.
    Mul = "mul", requires("m");
    /// Multiply signed by signed, high half of the product.
    Mulh = "mulh", requires("m");
    /// Multiply signed by unsigned, high half of the product.
    Mulhsu = "mulhsu", requires("m");
    /// Multiply unsigned by unsigned, high half of the product.
    Mulhu = "mulhu", requires("m");
    /// Divide, signed.
    Div = "div", requires("m");
    /// Divide, unsigned.
    Divu = "divu", requires("m");
    /// Remainder, signed.
    Rem = "rem", requires("m");
    /// Remainder, unsigned.
    Remu = "remu", requires("m");

    // RV64M
    /// Multiply words.
    Mulw = "mulw", requires("64-bit", "m");
    /// Divide words, signed.
    Divw = "divw", requires("64-bit", "m");
    /// Divide words, unsigned.
    Divuw = "divuw", requires("64-bit", "m");
    /// Remainder of words, signed.
    Remw = "remw", requires("64-bit", "m");
    /// Remainder of words, unsigned.
    Remuw = "remuw", requires("64-bit", "m");

    // A
    /// Load a word and reserve its address.
    LrW = "lr.w", requires("a");
    /// Store a word if the reservation on its address still holds.
    ScW = "sc.w", requires("a");
    /// Atomically swap a word in memory with a register.
    AmoswapW = "amoswap.w", requires("a");
    /// Atomically add a register to a word in memory.
    AmoaddW = "amoadd.w", requires("a");
    /// Atomically exclusive-or a register into a word in memory.
    AmoxorW = "amoxor.w", requires("a");
    /// Atomically and a register into a word in memory.
    AmoandW = "amoand.w", requires("a");
    /// Atomically or a register into a word in memory.
    AmoorW = "amoor.w", requires("a");
    /// Atomically replace a word in memory with the signed minimum of it and a register.
    AmominW = "amomin.w", requires("a");
    /// Atomically replace a word in memory with the signed maximum of it and a register.
    AmomaxW = "amomax.w", requires("a");
    /// Atomically replace a word in memory with the unsigned minimum of it and a register.
    AmominuW = "amominu.w", requires("a");
    /// Atomically replace a word in memory with the unsigned maximum of it and a register.
    AmomaxuW = "amomaxu.w", requires("a");

    // RV64A
    /// Load a doubleword and reserve its address.
    LrD = "lr.d", requires("64-bit", "a");
    /// Store a doubleword if the reservation on its address still holds.
    ScD = "sc.d", requires("64-bit", "a");
    /// Atomically swap a doubleword in memory with a register.
    AmoswapD = "amoswap.d", requires("64-bit", "a");
    /// Atomically add a register to a doubleword in memory.
    AmoaddD = "amoadd.d", requires("64-bit", "a");
    /// Atomically exclusive-or a register into a doubleword in memory.
    AmoxorD = "amoxor.d", requires("64-bit", "a");
    /// Atomically and a register into a doubleword in memory.
    AmoandD = "amoand.d", requires("64-bit", "a");
    /// Atomically or a register into a doubleword in memory.
    AmoorD = "amoor.d", requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the signed minimum of it and a register.
    AmominD = "amomin.d", requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the signed maximum of it and a register.
    AmomaxD = "amomax.d", requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the unsigned minimum of it and a register.
    AmominuD = "amominu.d", requires("64-bit", "a");
    /// Atomically replace a doubleword in memory with the unsigned maximum of it and a register.
    AmomaxuD = "amomaxu.d", requires("64-bit", "a");
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}
