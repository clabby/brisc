//! Checks that compiled code does exactly what the interpreter does.
//!
//! Every test runs a program twice, on the interpreter alone and with the JIT compiling loops as
//! soon as they repeat, and compares the two after every slice of the budget: the stop, the
//! registers, the program counter, the number of instructions retired and the memory.

use super::*;
use crate::memory::{Memory, SparseMemory};
use brisc_isa::{rv, rv_code, Format, Instruction, Opcode, XWord, REG_A0};

const CODE: XWord = 0x10000;
const DATA: XWord = 0x40000;
const PATCHED: i64 = rv!(addi a0, a0, 2) as i64;
const DATA_SIZE: usize = 0x4000;
const CODE_SIZE: usize = 0x2000;

#[derive(Debug, PartialEq)]
struct Snapshot {
    stop: Stop,
    registers: [XWord; 32],
    pc: XWord,
    instret: u64,
    memory: Vec<u8>,
    pages: usize,
}

/// A configuration that compiles inline as soon as a loop repeats, and optimizes after the given
/// number of instructions, if at all.
const fn inline(optimize_after: Option<u64>) -> JitConfig {
    JitConfig {
        threshold: 2,
        compilation: Compilation::Inline,
        max_region_instructions: 1500,
        optimize_after,
    }
}

fn machine(code: &[u8], registers: &[XWord; 32], jit: Option<JitConfig>) -> (Cpu, SparseMemory) {
    let mut memory = SparseMemory::new();
    memory.write(CODE, code).unwrap();
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let data: Vec<u8> = (0..DATA_SIZE).map(|_| rng.next() as u8).collect();
    memory.write(DATA, &data).unwrap();
    let mut cpu = Cpu::new(CODE);
    cpu.set_jit(jit);
    for (i, value) in registers.iter().enumerate() {
        cpu.registers.set(i, *value);
    }
    (cpu, memory)
}

fn snapshot(cpu: &Cpu, memory: &mut SparseMemory, stop: Stop) -> Snapshot {
    let mut image = vec![0; DATA_SIZE + CODE_SIZE];
    memory.read(DATA, &mut image[..DATA_SIZE]).unwrap();
    memory.read(CODE, &mut image[DATA_SIZE..]).unwrap();
    Snapshot {
        stop,
        registers: core::array::from_fn(|i| cpu.registers.get(i)),
        pc: cpu.pc,
        instret: cpu.instret,
        memory: image,
        pages: memory.page_count(),
    }
}

/// What a comparison found.
struct Outcome {
    /// How many instructions compiled code retired.
    compiled: u64,
    /// How many instructions the program retired.
    retired: u64,
    /// How the program stopped.
    stop: Stop,
}

/// Runs `code` on both engines in slices of the given sizes, until `total` instructions have
/// been retired or the program stops, and reports what happened.
fn compare(
    code: &[u8],
    registers: &[XWord; 32],
    slices: &[u64],
    total: u64,
    what: &str,
    config: JitConfig,
) -> Outcome {
    let (mut plain, mut plain_memory) = machine(code, registers, None);
    let (mut jitted, mut jitted_memory) = machine(code, registers, Some(config));
    let mut retired = 0;
    for slice in slices.iter().cycle() {
        let budget = (*slice).min(total - retired);
        let a = plain.run(&mut plain_memory, budget);
        let b = jitted.run(&mut jitted_memory, budget);
        let a = snapshot(&plain, &mut plain_memory, a);
        let b = snapshot(&jitted, &mut jitted_memory, b);
        assert_eq!(a, b, "{what}: after {} instructions", plain.instret);
        retired = plain.instret;
        if a.stop != Stop::Budget || retired >= total {
            return Outcome { compiled: jitted.jit_stats().retired, retired, stop: a.stop };
        }
    }
    unreachable!("the slices repeat")
}

#[test]
fn a_loop_sums_the_same() {
    let code = rv_code! {
        addi a0, zero, 0;
        addi a1, zero, 1000;
        add a0, a0, a1;
        addi a1, a1, -1;
        bne a1, zero, -8;
        ecall;
    };
    let registers = [0; 32];
    let outcome = compare(&code, &registers, &[u64::MAX], u64::MAX, "sum", inline(None));
    assert!(outcome.compiled > 2000, "the loop ran in compiled code: {}", outcome.compiled);
    for slice in [1, 2, 3, 5, 64, 1000, 2047, 2049, 4000] {
        compare(
            &code,
            &registers,
            &[slice],
            u64::MAX,
            &format!("sum in slices of {slice}"),
            inline(None),
        );
    }
}

#[test]
fn a_region_limit_of_zero_is_a_limit_of_one() {
    let code = rv_code! {
        addi a1, zero, 100;
        addi a0, a0, 1;
        bne a0, a1, -4;
        ecall;
    };
    let config = JitConfig { max_region_instructions: 0, ..inline(None) };
    compare(&code, &[0; 32], &[u64::MAX], u64::MAX, "no room in a region", config);
}

#[test]
fn code_that_a_compiled_loop_overwrites_runs_as_rewritten() {
    // The loop adds one to a0 per iteration. At the 200th iteration it overwrites that
    // instruction with one that adds two.
    let code = rv_code! {
        addi a1, zero, 300;
        auipc t0, 0;
        lui t1, {(PATCHED + 0x800) >> 12};
        addi t1, t1, {(PATCHED << 52) >> 52};
        addi a0, a0, 1;
        addi t2, t2, 1;
        addi t3, zero, 200;
        bne t2, t3, 8;
        sw t1, 12(t0);
        bne t2, a1, -20;
        ecall;
    };
    let (mut cpu, mut memory) = machine(&code, &[0; 32], Some(inline(None)));
    assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
    assert!(cpu.jit_stats().retired > 100, "the loop ran in compiled code");
    // 199 iterations add one, the 200th adds one before the rewrite, the last 100 add two.
    assert_eq!(cpu.registers.get(REG_A0), 200 + 2 * 100);

    for slice in [1, 7, 64, u64::MAX] {
        compare(
            &code,
            &[0; 32],
            &[slice],
            u64::MAX,
            &format!("rewrite in slices of {slice}"),
            inline(None),
        );
    }
}

#[test]
fn a_loop_that_stores_to_its_own_page_leaves_the_region() {
    // A store to a page that holds compiled code ends the region after the store.
    let code = rv_code! {
        addi a1, zero, 50;
        auipc t0, 0;
        sw a1, 512(t0);
        addi a1, a1, -1;
        bne a1, zero, -8;
        ecall;
    };
    compare(&code, &[0; 32], &[u64::MAX], u64::MAX, "stores in the code page", inline(None));
}

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    const fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const fn special_value(rng: &mut Rng) -> XWord {
    match rng.below(10) {
        0 => 0,
        1 => 1,
        2 => XWord::MAX,
        3 => i64::MIN as XWord,
        4 => i64::MAX as XWord,
        5 => 0x8000_0000,
        6 => 0xffff_ffff,
        7 => rng.below(64),
        _ => rng.next(),
    }
}

/// Generates a program: a mix of 32-bit and compressed instructions, with branches and jumps to
/// instruction boundaries of the program and memory accesses near the addresses that the base
/// registers hold.
fn random_program(rng: &mut Rng, len: usize) -> Vec<u8> {
    let mut instructions: Vec<(u32, Instruction)> = Vec::new();
    while instructions.len() < len {
        let compressed = rng.below(10) < 3;
        let word = if compressed {
            (rng.next() as u32 & 0xffff & !3) | rng.below(3) as u32
        } else {
            rng.next() as u32 | 3
        };
        let Ok(instruction) = Instruction::decode(word) else { continue };
        let mnemonic = instruction.opcode.mnemonic();
        let exotic = matches!(
            instruction.opcode,
            Opcode::Csrrw |
                Opcode::Csrrs |
                Opcode::Csrrc |
                Opcode::Csrrwi |
                Opcode::Csrrsi |
                Opcode::Csrrci |
                Opcode::Mret |
                Opcode::Sret |
                Opcode::Wfi |
                Opcode::SfenceVma |
                Opcode::FenceI
        ) || mnemonic.starts_with("lr") ||
            mnemonic.starts_with("sc") ||
            mnemonic.starts_with("amo");
        // Programs that stop at once are not interesting.
        if matches!(instruction.opcode, Opcode::Ecall | Opcode::Ebreak) ||
            (exotic && rng.below(40) != 0)
        {
            continue;
        }
        // Keep the base registers mostly intact so that memory accesses hit the data.
        if instruction.rd != 0 && instruction.rd < 12 && rng.below(8) != 0 {
            continue;
        }
        instructions.push((word, instruction));
    }

    // The offset in bytes from the start of the program of each instruction.
    let mut starts = vec![0];
    for (_, instruction) in &instructions {
        starts.push(starts.last().unwrap() + instruction.length as i64);
    }
    let mut code = Vec::new();
    for (index, (word, mut instruction)) in instructions.into_iter().enumerate() {
        let target = starts[rng.below(starts.len() as u64 - 1) as usize] - starts[index];
        let access = matches!(
            instruction.opcode,
            Opcode::Lb |
                Opcode::Lh |
                Opcode::Lw |
                Opcode::Ld |
                Opcode::Lbu |
                Opcode::Lhu |
                Opcode::Lwu |
                Opcode::Sb |
                Opcode::Sh |
                Opcode::Sw |
                Opcode::Sd
        );
        let word = match instruction.length {
            4 => {
                match instruction.opcode.format() {
                    Format::B | Format::J => instruction.imm = target as i32,
                    _ if access => {
                        instruction.rs1 = 1 + rng.below(4) as u8;
                        instruction.imm = rng.below(4096) as i32 - 2048;
                    }
                    _ => {}
                }
                instruction.encode().expect("a decoded instruction encodes")
            }
            _ => word,
        };
        code.extend(&word.to_le_bytes()[..instruction.length as usize]);
    }
    code.extend(rv!(ecall).to_le_bytes());
    code
}

#[test]
fn random_programs_run_alike() {
    let cases: u64 =
        std::env::var("BRISC_JIT_FUZZ").ok().and_then(|n| n.parse().ok()).unwrap_or(300);
    let (mut compiled, mut retired, mut stops) = (0, 0, std::collections::BTreeMap::new());
    for seed in 1..=cases {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let len = 16 + rng.below(48) as usize;
        let code = random_program(&mut rng, len);

        let mut registers = [0; 32];
        for (i, register) in registers.iter_mut().enumerate().skip(1) {
            *register = special_value(&mut rng);
            if i < 5 || (8..12).contains(&i) {
                // Near the end or the start of a page, to cross page boundaries.
                let offsets = [0x800, 0xffe, 0x1000, 0x1ff8, 0x2002, 0x3000 - 3];
                *register = DATA + offsets[rng.below(offsets.len() as u64) as usize];
            }
        }
        if rng.below(4) == 0 {
            // Stores into the code, which the interpreter has decoded.
            registers[3] = CODE + rng.below(code.len() as u64);
        }
        let slices = [1, 3, 7, 50, 500, u64::MAX];
        let slices = [slices[rng.below(6) as usize], slices[rng.below(6) as usize], u64::MAX];
        // Compile with optimizations at once, or quickly and never again, or quickly and
        // again after a few instructions.
        let config = inline([None, Some(u64::MAX), Some(200)][rng.below(3) as usize]);
        let outcome = compare(&code, &registers, &slices, 6000, &format!("seed {seed}"), config);
        compiled += outcome.compiled;
        retired += outcome.retired;
        let kind = match outcome.stop {
            Stop::Budget => "budget".to_string(),
            Stop::Fault(fault) => format!("{fault:?}").split('(').next().unwrap().to_string(),
            other => format!("{other:?}"),
        };
        *stops.entry(kind).or_insert(0) += 1;
    }
    assert!(compiled > 0, "some programs ran in compiled code");
    println!(
        "{cases} programs, {retired} instructions retired, {compiled} in compiled code: {stops:?}"
    );
}
