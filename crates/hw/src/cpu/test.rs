use super::*;
use crate::memory::SparseMemory;
use brisc_isa::{REG_A0, REG_A1, REG_ZERO};

const BASE: XWord = 0x1000;

fn addi(rd: usize, rs1: usize, imm: i32) -> Word {
    (imm as Word & 0xfff) << 20 | (rs1 as Word) << 15 | (rd as Word) << 7 | 0b001_0011
}

fn sw(rs2: usize, rs1: usize, imm: i32) -> Word {
    let imm = imm as Word & 0xfff;
    (imm >> 5) << 25 |
        (rs2 as Word) << 20 |
        (rs1 as Word) << 15 |
        0b010 << 12 |
        (imm & 0x1f) << 7 |
        0b010_0011
}

fn bne(rs1: usize, rs2: usize, offset: i32) -> Word {
    let imm = offset as Word;
    (imm >> 12 & 1) << 31 |
        (imm >> 5 & 0x3f) << 25 |
        (rs2 as Word) << 20 |
        (rs1 as Word) << 15 |
        0b001 << 12 |
        (imm >> 1 & 0xf) << 8 |
        (imm >> 11 & 1) << 7 |
        0b110_0011
}

fn jal(rd: usize, offset: i32) -> Word {
    let imm = offset as Word;
    (imm >> 20 & 1) << 31 |
        (imm >> 1 & 0x3ff) << 21 |
        (imm >> 11 & 1) << 20 |
        (imm >> 12 & 0xff) << 12 |
        (rd as Word) << 7 |
        0b110_1111
}

const ECALL: Word = 0b111_0011;

fn machine(address: XWord, program: &[Word]) -> (Cpu, SparseMemory) {
    let mut memory = SparseMemory::new();
    for (i, word) in program.iter().enumerate() {
        memory.store(address.wrapping_add(4 * i as XWord), word.to_le_bytes());
    }
    (Cpu::new(address), memory)
}

/// Counts `a0` down from `n` to zero, adding 3 to `a1` each time, then makes an `ecall`.
fn countdown(address: XWord, n: i32) -> (Cpu, SparseMemory) {
    machine(
        address,
        &[
            addi(REG_A0, REG_ZERO, n),
            addi(REG_A1, REG_A1, 3),
            addi(REG_A0, REG_A0, -1),
            bne(REG_A0, REG_ZERO, -8),
            ECALL,
        ],
    )
}

#[test]
fn runs_to_ecall() {
    let (mut cpu, mut memory) = countdown(BASE, 1000);
    assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
    assert_eq!(cpu.registers.get(REG_A0), 0);
    assert_eq!(cpu.registers.get(REG_A1), 3000);
    assert_eq!(cpu.pc, BASE + 16);
    assert_eq!(cpu.instret, 1 + 3 * 1000 + 1);
}

#[test]
fn retires_exactly_the_budget() {
    let (mut whole, mut memory) = countdown(BASE, 2000);
    assert_eq!(whole.run(&mut memory, u64::MAX), Stop::Ecall);

    for budget in [1, 2, 3, 7, SLOTS as u64 - 1, SLOTS as u64, SLOTS as u64 + 1, 5000] {
        let (mut cpu, mut memory) = countdown(BASE, 2000);
        let mut stop = Stop::Budget;
        while stop == Stop::Budget {
            let before = cpu.instret;
            stop = cpu.run(&mut memory, budget);
            assert!(cpu.instret - before == budget || stop == Stop::Ecall);
        }
        assert_eq!(stop, Stop::Ecall);
        assert_eq!(cpu.instret, whole.instret);
        assert_eq!(cpu.registers, whole.registers);
        assert_eq!(cpu.pc, whole.pc);
    }
}

#[test]
fn zero_budget_executes_nothing() {
    let (mut cpu, mut memory) = countdown(BASE, 5);
    assert_eq!(cpu.run(&mut memory, 0), Stop::Budget);
    assert_eq!((cpu.pc, cpu.instret), (BASE, 0));
}

#[test]
fn x0_stays_zero() {
    let (mut cpu, mut memory) = machine(
        BASE,
        &[addi(REG_ZERO, REG_ZERO, 5), jal(REG_ZERO, 4), addi(REG_A0, REG_ZERO, 1), ECALL],
    );
    cpu.registers.set(REG_ZERO, 9);
    assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
    assert_eq!(cpu.registers.get(REG_ZERO), 0);
    assert_eq!(cpu.registers.get(REG_A0), 1);
}

#[test]
fn executes_across_a_page_boundary() {
    // The second instruction straddles the boundary when the program starts six bytes
    // before it, and starts on it when the program starts four bytes before it.
    for start in [2 * BASE - 6, 2 * BASE - 4] {
        let (mut cpu, mut memory) = countdown(start, 10);
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        assert_eq!(cpu.registers.get(REG_A1), 30);
        assert_eq!(cpu.pc, start + 16);
    }
}

#[test]
fn overwritten_instructions_are_decoded_again() {
    // Runs the instruction at index `patched` once, overwrites it with the word in `t0`
    // through the address in `t1`, and runs it again.
    let (t0, t1) = (5, 6);
    let patched = 3;
    let program = [
        addi(REG_A1, REG_ZERO, 2),
        addi(REG_A1, REG_A1, -1),
        jal(REG_ZERO, 4),
        addi(REG_A0, REG_A0, 1),
        sw(t0, t1, 0),
        bne(REG_A1, REG_ZERO, -16),
        ECALL,
    ];

    // Once with the patched instruction inside a page and once with it straddling the
    // boundary to the next page.
    for start in [BASE, 2 * BASE - 4 * patched - 2] {
        let (mut cpu, mut memory) = machine(start, &program);
        cpu.registers.set(t0, addi(REG_A0, REG_A0, 100) as XWord);
        cpu.registers.set(t1, start + 4 * patched);
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        assert_eq!(cpu.registers.get(REG_A0), 101);
    }
}

#[test]
fn external_writes_are_seen_by_the_next_run() {
    let (mut cpu, mut memory) = machine(BASE, &[addi(REG_A0, REG_A0, 1), ECALL]);
    assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
    memory.write(BASE, &addi(REG_A0, REG_A0, 7).to_le_bytes());
    cpu.pc = BASE;
    assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
    assert_eq!(cpu.registers.get(REG_A0), 8);
}

#[test]
fn undecodable_word_faults_without_side_effects() {
    let (mut cpu, mut memory) = machine(BASE, &[addi(REG_A0, REG_ZERO, 1), 0xffff_ffff]);
    let stop = cpu.run(&mut memory, u64::MAX);
    assert!(matches!(stop, Stop::Fault(Fault::Decode(_))));
    assert_eq!((cpu.pc, cpu.instret), (BASE + 4, 1));
    // The fault is reported again when execution is retried.
    assert_eq!(cpu.run(&mut memory, u64::MAX), stop);
    assert_eq!((cpu.pc, cpu.instret), (BASE + 4, 1));
}
