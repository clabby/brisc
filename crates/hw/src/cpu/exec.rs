//! The interpreter loop.

use super::{
    code::{slot_of, CodePage, SLOTS},
    op::{code, Op},
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
use brisc_isa::Opcode;

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

/// Matches on an [Op::code].
///
/// The first block of arms is for the codes that are not instructions. Each arm of the second
/// block names one or more variants of [brisc_isa::Opcode] and runs its body for the two-byte
/// and for the four-byte form of those instructions, with `$advance` bound to the number of
/// slots the instruction occupies. That number is a constant in each copy of the body, which is
/// what keeps the address of the next [Op] independent of the current one.
///
/// The arms must cover exactly the instructions that the enabled features include.
macro_rules! dispatch {
    (
        $code:expr,
        $advance:ident,
        { $($internal:pat => $internal_body:block)* },
        { $($(#[$cfg:meta])* $($name:ident)|+ => $body:block)* }
    ) => {{
        /// The codes of the two-byte instructions.
        #[allow(non_upper_case_globals)]
        mod narrow {
            $($(#[$cfg])* codes!(2, $($name)+);)*
        }

        /// The codes of the four-byte instructions.
        #[allow(non_upper_case_globals)]
        mod wide {
            $($(#[$cfg])* codes!(4, $($name)+);)*
        }

        const _: () = {
            let mut i = 0;
            while i < brisc_isa::Opcode::ALL.len() {
                let opcode = brisc_isa::Opcode::ALL[i];
                let dispatched = match opcode {
                    $($(#[$cfg])* $(brisc_isa::Opcode::$name)|+ => true,)*
                    #[allow(unreachable_patterns)]
                    _ => false,
                };
                assert!(dispatched == opcode.is_supported());
                i += 1;
            }
        };

        match $code {
            $($internal => $internal_body)*
            $($(#[$cfg])* $(narrow::$name)|+ => {
                #[allow(unused_variables)]
                let $advance: usize = 1;
                $body
            })*
            $($(#[$cfg])* $(wide::$name)|+ => {
                #[allow(unused_variables)]
                let $advance: usize = 2;
                $body
            })*
            _ => unreachable!("slots only hold the codes of instructions that decode"),
        }
    }};
}

/// Defines a constant with the [Op::code] of each of the named instructions at this length.
macro_rules! codes {
    ($length:literal, $($name:ident)+) => {
        $(
            pub(super) const $name: u8 =
                $crate::cpu::op::Op::code(brisc_isa::Opcode::$name, $length);
        )+
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
            ($advance:ident) => {{
                match atomic(r, reservation, memory, op) {
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
            code::DECODE => { n += 1; break (Exit::Decode, pc!()); }
            code::PAGE_END => { n += 1; break (Exit::Leave, pc!()); }
            code::ILLEGAL => { n += 1; break (Exit::Illegal, pc!()); }
        }, {
            Ecall => { break (Exit::Ecall, pc!()); }

            Lui => { set!(advance, imm!()) }
            Auipc => { set!(advance, pc!().wrapping_add(imm!())) }

            Add => { set!(advance, rs1!().wrapping_add(rs2!())) }
            Sub => { set!(advance, rs1!().wrapping_sub(rs2!())) }
            Xor => { set!(advance, rs1!() ^ rs2!()) }
            Or => { set!(advance, rs1!() | rs2!()) }
            And => { set!(advance, rs1!() & rs2!()) }
            Sll => { set!(advance, rs1!().wrapping_shl(rs2!() as u32)) }
            Srl => { set!(advance, rs1!().wrapping_shr(rs2!() as u32)) }
            Sra => { set!(advance, (rs1!() as SXWord).wrapping_shr(rs2!() as u32) as XWord) }
            Slt => { set!(advance, ((rs1!() as SXWord) < (rs2!() as SXWord)) as XWord) }
            Sltu => { set!(advance, (rs1!() < rs2!()) as XWord) }

            Addi => { set!(advance, rs1!().wrapping_add(imm!())) }
            Xori => { set!(advance, rs1!() ^ imm!()) }
            Ori => { set!(advance, rs1!() | imm!()) }
            Andi => { set!(advance, rs1!() & imm!()) }
            Slli => { set!(advance, rs1!().wrapping_shl(op.imm as u32)) }
            Srli => { set!(advance, rs1!().wrapping_shr(op.imm as u32)) }
            Srai => { set!(advance, (rs1!() as SXWord).wrapping_shr(op.imm as u32) as XWord) }
            Slti => { set!(advance, ((rs1!() as SXWord) < (op.imm as SXWord)) as XWord) }
            Sltiu => { set!(advance, (rs1!() < imm!()) as XWord) }

            Lb => { load!(advance, i8) }
            Lh => { load!(advance, i16) }
            Lw => { load!(advance, i32) }
            Lbu => { load!(advance, u8) }
            Lhu => { load!(advance, u16) }
            #[cfg(feature = "64-bit")]
            Lwu => { load!(advance, u32) }
            #[cfg(feature = "64-bit")]
            Ld => { load!(advance, u64) }

            Sb => { store!(advance, u8) }
            Sh => { store!(advance, u16) }
            Sw => { store!(advance, u32) }
            #[cfg(feature = "64-bit")]
            Sd => { store!(advance, u64) }

            Beq => { branch!(advance, rs1!() == rs2!()) }
            Bne => { branch!(advance, rs1!() != rs2!()) }
            Blt => { branch!(advance, (rs1!() as SXWord) < (rs2!() as SXWord)) }
            Bge => { branch!(advance, (rs1!() as SXWord) >= (rs2!() as SXWord)) }
            Bltu => { branch!(advance, rs1!() < rs2!()) }
            Bgeu => { branch!(advance, rs1!() >= rs2!()) }

            Jal => { jump!(advance, pc!().wrapping_add(imm!())) }
            Jalr => { jump!(advance, address!() & !1) }

            #[cfg(feature = "64-bit")]
            Addw => { set_word!(advance, (rs1!() as Word).wrapping_add(rs2!() as Word)) }
            #[cfg(feature = "64-bit")]
            Subw => { set_word!(advance, (rs1!() as Word).wrapping_sub(rs2!() as Word)) }
            #[cfg(feature = "64-bit")]
            Sllw => { set_word!(advance, (rs1!() as Word).wrapping_shl(rs2!() as u32)) }
            #[cfg(feature = "64-bit")]
            Srlw => { set_word!(advance, (rs1!() as Word).wrapping_shr(rs2!() as u32)) }
            #[cfg(feature = "64-bit")]
            Sraw => { set_word!(advance, (rs1!() as i32).wrapping_shr(rs2!() as u32) as Word) }
            #[cfg(feature = "64-bit")]
            Addiw => { set_word!(advance, (rs1!() as Word).wrapping_add(op.imm as Word)) }
            #[cfg(feature = "64-bit")]
            Slliw => { set_word!(advance, (rs1!() as Word).wrapping_shl(op.imm as u32)) }
            #[cfg(feature = "64-bit")]
            Srliw => { set_word!(advance, (rs1!() as Word).wrapping_shr(op.imm as u32)) }
            #[cfg(feature = "64-bit")]
            Sraiw => { set_word!(advance, (rs1!() as i32).wrapping_shr(op.imm as u32) as Word) }

            #[cfg(feature = "m")]
            Mul => { set!(advance, rs1!().wrapping_mul(rs2!())) }
            #[cfg(feature = "m")]
            Mulh => {
                let product = (rs1!() as SXWord as SDoubleXWord) * (rs2!() as SXWord as SDoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            #[cfg(feature = "m")]
            Mulhsu => {
                let product = (rs1!() as SXWord as SDoubleXWord)
                    .wrapping_mul(rs2!() as DoubleXWord as SDoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            #[cfg(feature = "m")]
            Mulhu => {
                let product = (rs1!() as DoubleXWord) * (rs2!() as DoubleXWord);
                set!(advance, (product >> X_LEN) as XWord)
            }
            // Division by zero and signed overflow do not trap: they produce the results the
            // M extension defines for them.
            #[cfg(feature = "m")]
            Div => {
                let (dividend, divisor) = (rs1!() as SXWord, rs2!() as SXWord);
                set!(advance, if divisor == 0 { XWord::MAX } else { dividend.wrapping_div(divisor) as XWord })
            }
            #[cfg(feature = "m")]
            Divu => { set!(advance, rs1!().checked_div(rs2!()).unwrap_or(XWord::MAX)) }
            #[cfg(feature = "m")]
            Rem => {
                let (dividend, divisor) = (rs1!() as SXWord, rs2!() as SXWord);
                set!(advance, if divisor == 0 { dividend as XWord } else { dividend.wrapping_rem(divisor) as XWord })
            }
            #[cfg(feature = "m")]
            Remu => { set!(advance, rs1!().checked_rem(rs2!()).unwrap_or(rs1!())) }

            #[cfg(all(feature = "64-bit", feature = "m"))]
            Mulw => { set_word!(advance, (rs1!() as Word).wrapping_mul(rs2!() as Word)) }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            Divw => {
                let (dividend, divisor) = (rs1!() as i32, rs2!() as i32);
                set_word!(advance, if divisor == 0 { Word::MAX } else { dividend.wrapping_div(divisor) as Word })
            }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            Divuw => { set_word!(advance, (rs1!() as Word).checked_div(rs2!() as Word).unwrap_or(Word::MAX)) }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            Remw => {
                let (dividend, divisor) = (rs1!() as i32, rs2!() as i32);
                set_word!(advance, if divisor == 0 { dividend as Word } else { dividend.wrapping_rem(divisor) as Word })
            }
            #[cfg(all(feature = "64-bit", feature = "m"))]
            Remuw => { set_word!(advance, (rs1!() as Word).checked_rem(rs2!() as Word).unwrap_or(rs1!() as Word)) }

            #[cfg(feature = "a")]
            LrW | ScW | AmoswapW | AmoaddW | AmoxorW | AmoandW | AmoorW | AmominW | AmomaxW
                | AmominuW | AmomaxuW => { atomic!(advance) }
            #[cfg(all(feature = "64-bit", feature = "a"))]
            LrD | ScD | AmoswapD | AmoaddD | AmoxorD | AmoandD | AmoorD | AmominD | AmomaxD
                | AmominuD | AmomaxuD => { atomic!(advance) }

            // Breakpoints, privilege levels and interrupts are not modelled, and with a single
            // hart and decoded code that is kept coherent with memory there is nothing to order
            // or synchronize. Programs execute these rarely, so they are kept out of the way of
            // the other instructions.
            Ebreak | Fence | FenceI | Sret | Mret | Wfi | SfenceVma => {
                core::hint::cold_path();
                slot += advance;
            }
            // No control and status register is modelled: all of them read as zero and ignore
            // writes.
            Csrrw | Csrrs | Csrrc | Csrrwi | Csrrsi | Csrrci => {
                core::hint::cold_path();
                set!(advance, 0)
            }
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
    r: &mut [XWord; Registers::LEN],
    reservation: &mut Option<Address>,
    memory: &mut M,
    op: Op,
) -> Result<bool, Fault> {
    use Opcode::*;

    let opcode = op.opcode();
    let address = r[Registers::slot(op.rs1)];
    let source = r[Registers::slot(op.rs2)];
    // The access is a word, or else a doubleword.
    let word = matches!(
        opcode,
        LrW | ScW |
            AmoswapW |
            AmoaddW |
            AmoxorW |
            AmoandW |
            AmoorW |
            AmominW |
            AmomaxW |
            AmominuW |
            AmomaxuW
    );
    if address & (if word { 3 } else { 7 }) != 0 {
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

    let (result, watched) = match opcode {
        LrW | LrD => {
            *reservation = Some(address);
            (load(memory), false)
        }
        ScW | ScD => {
            let reserved = reservation.take() == Some(address);
            let watched = reserved && store(memory, source);
            (!reserved as XWord, watched)
        }
        _ => {
            let old = load(memory);
            let source = if word { source as i32 as SXWord as XWord } else { source };
            let new = match opcode {
                AmoswapW | AmoswapD => source,
                AmoaddW | AmoaddD => old.wrapping_add(source),
                AmoxorW | AmoxorD => old ^ source,
                AmoandW | AmoandD => old & source,
                AmoorW | AmoorD => old | source,
                AmominW | AmominD => (old as SXWord).min(source as SXWord) as XWord,
                AmomaxW | AmomaxD => (old as SXWord).max(source as SXWord) as XWord,
                AmominuW | AmominuD => old.min(source),
                AmomaxuW | AmomaxuD => old.max(source),
                _ => unreachable!("only atomic instructions are dispatched here"),
            };
            (old, store(memory, new))
        }
    };

    r[Registers::slot(op.rd)] = result;
    Ok(watched)
}
