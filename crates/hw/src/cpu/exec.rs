//! The interpreter loop.

use super::{
    code::{slot_of, CodePage, SLOTS},
    op::{narrow, wide, Op},
    Registers,
};
use crate::memory::{page_index, Memory};
use brisc_isa::{SXWord, XWord};

#[cfg(feature = "64-bit")]
use brisc_isa::Word;

#[cfg(feature = "a")]
use super::Fault;
#[cfg(feature = "a")]
use crate::memory::Address;
#[cfg(feature = "a")]
use brisc_isa::AmoFunction;

#[cfg(feature = "m")]
use brisc_isa::{DoubleXWord, SDoubleXWord, X_LEN};

/// Why [execute] returned.
pub(super) enum Exit {
    /// Execution continues at the returned program counter, on another page or after the caller
    /// has looked at the remaining budget and at writes to watched pages.
    Leave,
    /// The slot at the returned program counter has not been decoded.
    Decode,
    /// The instruction at the returned program counter is an `ecall`. It has been retired.
    Ecall,
    /// The word at the returned program counter is not an instruction.
    Illegal,
    /// The instruction at the returned program counter faulted.
    #[cfg(feature = "a")]
    Fault(Fault),
}

/// Matches on an [Op::code], running `$body` for the two-byte and the four-byte form of each
/// listed opcode with `$advance` bound to the number of slots the instruction occupies.
macro_rules! dispatch {
    ($code:expr, $advance:ident, { $($(#[$cfg:meta])* $name:ident => $body:block)* }) => {
        match $code {
            $($(#[$cfg])* narrow::$name => {
                #[allow(unused_variables)]
                let $advance: usize = 1;
                $body
            })*
            $($(#[$cfg])* wide::$name => {
                #[allow(unused_variables)]
                let $advance: usize = 2;
                $body
            })*
            _ => unreachable!("slots only hold opcodes produced by lowering"),
        }
    };
}

/// Executes the instructions of one page of decoded `code`, starting at `slot`, until execution
/// leaves the page or has to stop. `base` is the address of the page.
///
/// Returns why execution stopped and the program counter at that point, and subtracts the number
/// of instructions retired from `remaining`.
///
/// With `PRECISE` set, the budget is checked before every instruction and exactly `remaining`
/// instructions are retired at most. Without it, the budget is only checked after control
/// transfers, which is sound when `remaining` is at least [SLOTS]: the loop returns once fewer
/// than [SLOTS] instructions remain, and no more than [SLOTS] instructions can execute between
/// two checks because every instruction advances by at least one slot.
#[allow(
    clippy::unnecessary_cast,
    reason = "XWord is 32 or 64 bits wide depending on the enabled features"
)]
pub(super) fn execute<M: Memory, const PRECISE: bool>(
    registers: &mut Registers,
    #[cfg(feature = "a")] reservation: &mut Option<Address>,
    memory: &mut M,
    code: &CodePage,
    base: XWord,
    mut slot: usize,
    remaining: &mut u64,
) -> (Exit, XWord) {
    let r = &mut registers.0;
    let budget = (*remaining).min(i64::MAX as u64) as i64;
    let mut n = budget;

    macro_rules! pc {
        () => {
            base.wrapping_add((slot as XWord) << 1)
        };
    }
    // Leaves the loop after a control transfer if the remaining budget no longer covers a
    // straight run through the page.
    macro_rules! check_budget {
        () => {
            if !PRECISE && n < SLOTS as i64 {
                break (Exit::Leave, pc!());
            }
        };
    }

    let exit = loop {
        if PRECISE && n == 0 {
            break (Exit::Leave, pc!());
        }
        let op: Op = code[slot];
        n -= 1;

        macro_rules! rs1 {
            () => {
                r[Registers::slot(op.rs1)]
            };
        }
        macro_rules! rs2 {
            () => {
                r[Registers::slot(op.rs2)]
            };
        }
        macro_rules! imm {
            () => {
                op.imm as SXWord as XWord
            };
        }
        macro_rules! address {
            () => {
                rs1!().wrapping_add(imm!())
            };
        }
        // Writes the destination register and moves to the next instruction.
        macro_rules! set {
            ($advance:ident, $value:expr) => {{
                let value: XWord = $value;
                r[Registers::slot(op.rd)] = value;
                slot += $advance;
            }};
        }
        // Sign-extends a 32-bit result into the destination register.
        #[cfg(feature = "64-bit")]
        macro_rules! set_word {
            ($advance:ident, $value:expr) => {{
                let value: Word = $value;
                set!($advance, value as i32 as SXWord as XWord)
            }};
        }
        macro_rules! load {
            ($advance:ident, $ty:ty) => {
                set!($advance, <$ty>::from_le_bytes(memory.load(address!())) as SXWord as XWord)
            };
        }
        macro_rules! store {
            ($advance:ident, $ty:ty) => {{
                let watched = memory.store(address!(), (rs2!() as $ty).to_le_bytes());
                slot += $advance;
                if watched {
                    break (Exit::Leave, pc!());
                }
            }};
        }
        macro_rules! branch {
            ($advance:ident, $taken:expr) => {{
                if $taken {
                    let target = slot.wrapping_add_signed((op.imm >> 1) as isize);
                    if target >= SLOTS {
                        break (Exit::Leave, pc!().wrapping_add(imm!()));
                    }
                    slot = target;
                } else {
                    slot += $advance;
                }
                check_budget!();
            }};
        }
        // Links the destination register and continues at `$target`.
        macro_rules! jump {
            ($advance:ident, $target:expr) => {{
                let target: XWord = $target;
                r[Registers::slot(op.rd)] = pc!().wrapping_add(($advance as XWord) << 1);
                if page_index(target) != page_index(base) {
                    break (Exit::Leave, target);
                }
                slot = slot_of(target);
                check_budget!();
            }};
        }
        #[cfg(feature = "a")]
        macro_rules! atomic {
            ($advance:ident, $function:expr) => {{
                match atomic($function, r, reservation, memory, op) {
                    Ok(watched) => {
                        slot += $advance;
                        if watched {
                            break (Exit::Leave, pc!());
                        }
                    }
                    Err(fault) => {
                        n += 1;
                        break (Exit::Fault(fault), pc!());
                    }
                }
            }};
        }

        dispatch!(op.code, advance, {
            DECODE => { n += 1; break (Exit::Decode, pc!()); }
            PAGE_END => { n += 1; break (Exit::Leave, pc!()); }
            ILLEGAL => { n += 1; break (Exit::Illegal, pc!()); }
            ECALL => { break (Exit::Ecall, pc!()); }

            LI => { set!(advance, imm!()) }
            AUIPC => { set!(advance, pc!().wrapping_add(imm!())) }

            ADD => { set!(advance, rs1!().wrapping_add(rs2!())) }
            SUB => { set!(advance, rs1!().wrapping_sub(rs2!())) }
            XOR => { set!(advance, rs1!() ^ rs2!()) }
            OR => { set!(advance, rs1!() | rs2!()) }
            AND => { set!(advance, rs1!() & rs2!()) }
            SLL => { set!(advance, rs1!().wrapping_shl(rs2!() as u32)) }
            SRL => { set!(advance, rs1!().wrapping_shr(rs2!() as u32)) }
            SRA => { set!(advance, (rs1!() as SXWord).wrapping_shr(rs2!() as u32) as XWord) }
            SLT => { set!(advance, ((rs1!() as SXWord) < (rs2!() as SXWord)) as XWord) }
            SLTU => { set!(advance, (rs1!() < rs2!()) as XWord) }

            ADDI => { set!(advance, rs1!().wrapping_add(imm!())) }
            XORI => { set!(advance, rs1!() ^ imm!()) }
            ORI => { set!(advance, rs1!() | imm!()) }
            ANDI => { set!(advance, rs1!() & imm!()) }
            SLLI => { set!(advance, rs1!().wrapping_shl(op.imm as u32)) }
            SRLI => { set!(advance, rs1!().wrapping_shr(op.imm as u32)) }
            SRAI => { set!(advance, (rs1!() as SXWord).wrapping_shr(op.imm as u32) as XWord) }
            SLTI => { set!(advance, ((rs1!() as SXWord) < (op.imm as SXWord)) as XWord) }
            SLTIU => { set!(advance, (rs1!() < imm!()) as XWord) }

            LB => { load!(advance, i8) }
            LH => { load!(advance, i16) }
            LW => { load!(advance, i32) }
            LBU => { load!(advance, u8) }
            LHU => { load!(advance, u16) }
            #[cfg(feature = "64-bit")]
            LWU => { load!(advance, u32) }
            #[cfg(feature = "64-bit")]
            LD => { load!(advance, u64) }

            SB => { store!(advance, u8) }
            SH => { store!(advance, u16) }
            SW => { store!(advance, u32) }
            #[cfg(feature = "64-bit")]
            SD => { store!(advance, u64) }

            BEQ => { branch!(advance, rs1!() == rs2!()) }
            BNE => { branch!(advance, rs1!() != rs2!()) }
            BLT => { branch!(advance, (rs1!() as SXWord) < (rs2!() as SXWord)) }
            BGE => { branch!(advance, (rs1!() as SXWord) >= (rs2!() as SXWord)) }
            BLTU => { branch!(advance, rs1!() < rs2!()) }
            BGEU => { branch!(advance, rs1!() >= rs2!()) }

            JAL => { jump!(advance, pc!().wrapping_add(imm!())) }
            JALR => { jump!(advance, address!() & !1) }

            #[cfg(feature = "64-bit")]
            ADDW => { set_word!(advance, (rs1!() as Word).wrapping_add(rs2!() as Word)) }
            #[cfg(feature = "64-bit")]
            SUBW => { set_word!(advance, (rs1!() as Word).wrapping_sub(rs2!() as Word)) }
            #[cfg(feature = "64-bit")]
            SLLW => { set_word!(advance, (rs1!() as Word).wrapping_shl(rs2!() as u32)) }
            #[cfg(feature = "64-bit")]
            SRLW => { set_word!(advance, (rs1!() as Word).wrapping_shr(rs2!() as u32)) }
            #[cfg(feature = "64-bit")]
            SRAW => { set_word!(advance, (rs1!() as i32).wrapping_shr(rs2!() as u32) as Word) }
            #[cfg(feature = "64-bit")]
            ADDIW => { set_word!(advance, (rs1!() as Word).wrapping_add(op.imm as Word)) }
            #[cfg(feature = "64-bit")]
            SLLIW => { set_word!(advance, (rs1!() as Word).wrapping_shl(op.imm as u32)) }
            #[cfg(feature = "64-bit")]
            SRLIW => { set_word!(advance, (rs1!() as Word).wrapping_shr(op.imm as u32)) }
            #[cfg(feature = "64-bit")]
            SRAIW => { set_word!(advance, (rs1!() as i32).wrapping_shr(op.imm as u32) as Word) }

            #[cfg(feature = "m")]
            MUL => { set!(advance, rs1!().wrapping_mul(rs2!())) }
            #[cfg(feature = "m")]
            MULH => {
                let product = (rs1!() as SXWord as SDoubleXWord) * (rs2!() as SXWord as SDoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            #[cfg(feature = "m")]
            MULHSU => {
                let product = (rs1!() as SXWord as SDoubleXWord)
                    .wrapping_mul(rs2!() as DoubleXWord as SDoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            #[cfg(feature = "m")]
            MULHU => {
                let product = (rs1!() as DoubleXWord) * (rs2!() as DoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            // Division by zero and signed overflow do not trap: they produce the results the
            // M extension defines for them.
            #[cfg(feature = "m")]
            DIV => {
                let (dividend, divisor) = (rs1!() as SXWord, rs2!() as SXWord);
                set!(advance, if divisor == 0 { XWord::MAX } else { dividend.wrapping_div(divisor) as XWord })
            }
            #[cfg(feature = "m")]
            DIVU => { set!(advance, rs1!().checked_div(rs2!()).unwrap_or(XWord::MAX)) }
            #[cfg(feature = "m")]
            REM => {
                let (dividend, divisor) = (rs1!() as SXWord, rs2!() as SXWord);
                set!(advance, if divisor == 0 { dividend as XWord } else { dividend.wrapping_rem(divisor) as XWord })
            }
            #[cfg(feature = "m")]
            REMU => { set!(advance, rs1!().checked_rem(rs2!()).unwrap_or(rs1!())) }

            #[cfg(all(feature = "64-bit", feature = "m"))]
            MULW => { set_word!(advance, (rs1!() as Word).wrapping_mul(rs2!() as Word)) }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            DIVW => {
                let (dividend, divisor) = (rs1!() as i32, rs2!() as i32);
                set_word!(advance, if divisor == 0 { Word::MAX } else { dividend.wrapping_div(divisor) as Word })
            }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            DIVUW => { set_word!(advance, (rs1!() as Word).checked_div(rs2!() as Word).unwrap_or(Word::MAX)) }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            REMW => {
                let (dividend, divisor) = (rs1!() as i32, rs2!() as i32);
                set_word!(advance, if divisor == 0 { dividend as Word } else { dividend.wrapping_rem(divisor) as Word })
            }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            REMUW => { set_word!(advance, (rs1!() as Word).checked_rem(rs2!() as Word).unwrap_or(rs1!() as Word)) }

            #[cfg(feature = "a")]
            LR => { atomic!(advance, AmoFunction::Lr) }
            #[cfg(feature = "a")]
            SC => { atomic!(advance, AmoFunction::Sc) }
            #[cfg(feature = "a")]
            AMOSWAP => { atomic!(advance, AmoFunction::Amoswap) }
            #[cfg(feature = "a")]
            AMOADD => { atomic!(advance, AmoFunction::Amoadd) }
            #[cfg(feature = "a")]
            AMOXOR => { atomic!(advance, AmoFunction::Amoxor) }
            #[cfg(feature = "a")]
            AMOAND => { atomic!(advance, AmoFunction::Amoand) }
            #[cfg(feature = "a")]
            AMOOR => { atomic!(advance, AmoFunction::Amoor) }
            #[cfg(feature = "a")]
            AMOMIN => { atomic!(advance, AmoFunction::Amomin) }
            #[cfg(feature = "a")]
            AMOMAX => { atomic!(advance, AmoFunction::Amomax) }
            #[cfg(feature = "a")]
            AMOMINU => { atomic!(advance, AmoFunction::Amominu) }
            #[cfg(feature = "a")]
            AMOMAXU => { atomic!(advance, AmoFunction::Amomaxu) }
        })
    };

    *remaining -= (budget - n) as u64;
    exit
}

/// Executes an instruction of the A extension. Returns whether it wrote to a watched page.
#[cfg(feature = "a")]
#[cold]
#[inline(never)]
#[allow(
    clippy::unnecessary_cast,
    reason = "XWord is 32 or 64 bits wide depending on the enabled features"
)]
fn atomic<M: Memory>(
    function: AmoFunction,
    r: &mut [XWord; Registers::LEN],
    reservation: &mut Option<Address>,
    memory: &mut M,
    op: Op,
) -> Result<bool, Fault> {
    let address = r[Registers::slot(op.rs1)];
    let source = r[Registers::slot(op.rs2)];
    // Lowering only produces sizes of 4 and, on 64-bit, 8 bytes.
    let word = op.imm == 4;
    if address & (op.imm as XWord - 1) != 0 {
        return Err(Fault::UnalignedAmo);
    }

    let load = |memory: &mut M| -> XWord {
        #[cfg(feature = "64-bit")]
        if !word {
            return XWord::from_le_bytes(memory.load(address));
        }
        i32::from_le_bytes(memory.load(address)) as SXWord as XWord
    };
    let store = |memory: &mut M, value: XWord| -> bool {
        #[cfg(feature = "64-bit")]
        if !word {
            return memory.store(address, value.to_le_bytes());
        }
        memory.store(address, (value as u32).to_le_bytes())
    };

    let (result, watched) = match function {
        AmoFunction::Lr => {
            *reservation = Some(address);
            // A 32-bit load-reserved zero-extends its result.
            let value = if word { load(memory) as u32 as XWord } else { load(memory) };
            (value, false)
        }
        AmoFunction::Sc => {
            let reserved = reservation.take() == Some(address);
            let watched = reserved && store(memory, source);
            (!reserved as XWord, watched)
        }
        _ => {
            let old = load(memory);
            let source = if word { source as i32 as SXWord as XWord } else { source };
            let new = match function {
                AmoFunction::Amoswap => source,
                AmoFunction::Amoadd => old.wrapping_add(source),
                AmoFunction::Amoxor => old ^ source,
                AmoFunction::Amoand => old & source,
                AmoFunction::Amoor => old | source,
                AmoFunction::Amomin => (old as SXWord).min(source as SXWord) as XWord,
                AmoFunction::Amomax => (old as SXWord).max(source as SXWord) as XWord,
                AmoFunction::Amominu => old.min(source),
                AmoFunction::Amomaxu => old.max(source),
                AmoFunction::Lr | AmoFunction::Sc => unreachable!("handled above"),
            };
            (old, store(memory, new))
        }
    };

    r[Registers::slot(op.rd)] = result;
    Ok(watched)
}
