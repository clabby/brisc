//! The connection between the interpreter and the JIT compiler.
//!
//! The interpreter counts how often control flows back to each slot of a page. When the count of a
//! slot reaches the threshold of the [`JitConfig`], the interpreter returns to [`Cpu::run`], which
//! has the region of the page that starts at the slot compiled and, from then on, runs the compiled
//! code whenever control flows back to the slot or to any other place that the region can be
//! entered. The compiled code leaves for the interpreter wherever it meets an instruction that it
//! does not contain.
//!
//! Regions are compiled twice, first quickly and, once a region has proved to be hot, again with
//! optimizations. Either happens on a thread of its own by default, so the interpreter keeps
//! running while it waits for the code.

use super::{
    code::{CodePage, SLOTS},
    op::{code, Op},
    Cpu,
};
use crate::memory::{page_base, Memory, PageIndex};
use brisc_isa::Instruction;
use brisc_jit::{Background, Ctx, Jit, Job, Quality, Region, Request};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// The table entry of a slot that no region has been tried for.
const UNKNOWN: u32 = 0;

/// The table entry of a slot that no region can be compiled for.
const FAILED: u32 = 1;

/// The table entry of a slot whose region is being compiled.
const PENDING: u32 = 2;

/// The table entry of a slot with a region is this plus the index of the region.
const COMPILED: u32 = 3;

/// The largest number of instructions that a region compiled quickly runs before it leaves. Its
/// code is replaced when a better version is ready, but a region that never leaves would never
/// run the new one.
const QUICK_QUANTUM: u64 = 100_000;

/// Where regions are compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compilation {
    /// On a thread of its own. The interpreter runs the loop that the region replaces until the
    /// code is ready, which for a short-lived program may be never.
    Background,
    /// On the thread of the hart, which waits for the code. The outcome does not depend on timing,
    /// so this is the choice for tests.
    Inline,
}

/// How the hart uses the JIT compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitConfig {
    /// How many times control has to flow back to a slot before a region is compiled there.
    /// At least one.
    pub threshold: u16,
    /// Where regions are compiled.
    pub compilation: Compilation,
    /// The largest number of instructions in a region, at least one. Control flow beyond that is
    /// left to the interpreter.
    pub max_region_instructions: usize,
    /// The number of instructions that a region has to retire before it is compiled again with
    /// optimizations, or None to compile it with optimizations in the first place. Optimizing
    /// takes several times as long as compiling quickly, and pays off for code that runs for
    /// long.
    pub optimize_after: Option<u64>,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            threshold: 2000,
            compilation: Compilation::Background,
            max_region_instructions: 1500,
            optimize_after: Some(500_000),
        }
    }
}

/// What the JIT has done.
#[derive(Debug, Clone, Copy, Default)]
pub struct JitStats {
    /// The number of regions compiled, counting the ones that were compiled again.
    pub compiled: usize,
    /// The time spent compiling, on whichever thread.
    pub compile_time: Duration,
    /// How often a region was entered.
    pub entries: u64,
    /// The number of instructions that regions retired.
    pub retired: u64,
}

/// A region that has been compiled.
struct Compiled {
    region: Region,
    quality: Quality,
    /// The position of the page of the region in the code cache.
    position: usize,
    /// The slot at which the region was compiled to start.
    header: usize,
    /// The number of instructions that the region has retired.
    retired: u64,
    /// Whether a better version has been asked for.
    improving: bool,
}

/// What a job that was given to the compiler thread was for.
struct Pending {
    /// The position of the page in the code cache.
    position: usize,
    slot: usize,
    /// The quality that the job asked for.
    quality: Quality,
    /// The generation of the page when the job was submitted.
    generation: u64,
    /// The index of the region that the job improves, if it does.
    improves: Option<usize>,
}

/// The regions of a hart and the means to compile them.
pub(super) struct JitState {
    config: Option<JitConfig>,
    /// The compilers that run on the thread of the hart, for [`Quality::Quick`] and
    /// [`Quality::Fast`]. A compiler that the host does not support is None.
    inline: [Option<Option<Jit>>; 2],
    /// The thread that compiles regions in the background.
    worker: Option<Background>,
    /// The jobs that the thread has not finished, by identifier.
    pending: HashMap<u64, Pending>,
    /// The identifier of the next job.
    next_job: u64,
    /// How often each page of the code cache has been invalidated.
    generations: Vec<u64>,
    regions: Vec<Compiled>,
    /// What is known about the region that starts at each slot of each page of the code cache:
    /// [`UNKNOWN`], [`FAILED`], [`PENDING`], or [`COMPILED`] plus the index in [`Self::regions`]
    /// of the region.
    table: Vec<Box<[u32; SLOTS]>>,
    stats: JitStats,
}

impl core::fmt::Debug for JitState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JitState")
            .field("config", &self.config)
            .field("regions", &self.regions.len())
            .finish_non_exhaustive()
    }
}

impl JitState {
    pub(super) fn new() -> Self {
        Self {
            config: Some(JitConfig::default()),
            inline: [None, None],
            worker: None,
            pending: HashMap::new(),
            next_job: 0,
            generations: Vec::new(),
            regions: Vec::new(),
            table: Vec::new(),
            stats: JitStats::default(),
        }
    }

    pub(super) const fn enabled(&self) -> bool {
        self.config.is_some()
    }

    /// The number of times control flows back to a slot before the interpreter asks for a region.
    pub(super) const fn threshold(&self) -> u16 {
        match self.config {
            Some(config) => config.threshold,
            None => u16::MAX,
        }
    }

    pub(super) fn set_config(&mut self, config: Option<JitConfig>) {
        self.config = config.map(|config| JitConfig {
            threshold: config.threshold.max(1),
            max_region_instructions: config.max_region_instructions.max(1),
            ..config
        });
    }

    pub(super) const fn config(&self) -> Option<JitConfig> {
        self.config
    }

    pub(super) const fn stats(&self) -> JitStats {
        self.stats
    }

    /// Forgets every region. Regions that are being compiled are ignored when they finish.
    pub(super) fn reset(&mut self) {
        for table in &mut self.table {
            table.fill(UNKNOWN);
        }
        for generation in &mut self.generations {
            *generation += 1;
        }
    }

    /// Forgets the regions of the page at position in the code cache.
    pub(super) fn invalidate(&mut self, position: usize) {
        if let Some(table) = self.table.get_mut(position) {
            table.fill(UNKNOWN);
            self.generations[position] += 1;
        }
    }

    fn ensure_page(&mut self, position: usize) {
        while self.table.len() <= position {
            self.table.push(Box::new([UNKNOWN; SLOTS]));
            self.generations.push(0);
        }
    }

    /// Returns the quality of a region that is compiled for the first time.
    const fn first_quality(config: JitConfig) -> Quality {
        if config.optimize_after.is_some() {
            Quality::Quick
        } else {
            Quality::Fast
        }
    }

    /// Lets the slots at which the region with the given index can be entered lead to it, unless
    /// they lead to another region already.
    fn map_entries(&mut self, index: usize) {
        let Compiled { region, position, .. } = &self.regions[index];
        let table = &mut self.table[*position];
        for slot in region.entries().iter() {
            if table[*slot] < COMPILED {
                table[*slot] = COMPILED + index as u32;
            }
        }
    }

    /// Records a region that has been compiled for a slot, or that cannot be.
    fn install(&mut self, position: usize, slot: usize, region: Option<Region>, quality: Quality) {
        let Some(region) = region else {
            self.table[position][slot] = FAILED;
            return;
        };
        self.regions.push(Compiled {
            region,
            quality,
            position,
            header: slot,
            retired: 0,
            improving: false,
        });
        self.stats.compiled += 1;
        self.map_entries(self.regions.len() - 1);
    }

    /// Replaces the region with the given index by a better one.
    fn replace(&mut self, index: usize, region: Region) {
        let compiled = &mut self.regions[index];
        let old = core::mem::replace(&mut compiled.region, region);
        compiled.quality = Quality::Fast;
        let position = compiled.position;
        // The new region may not have all the entries of the old one.
        for slot in old.entries().iter() {
            if self.table[position][*slot] == COMPILED + index as u32 {
                self.table[position][*slot] = UNKNOWN;
            }
        }
        self.stats.compiled += 1;
        self.map_entries(index);
    }

    /// Takes in what the compiler thread has finished.
    fn collect(&mut self) {
        let Some(worker) = &self.worker else { return };
        for done in worker.finished().collect::<Vec<_>>() {
            self.stats.compile_time += done.compile_time;
            let Some(pending) = self.pending.remove(&done.id) else { continue };
            // The page may have changed since the job was submitted.
            if self.generations[pending.position] != pending.generation {
                continue;
            }
            let Pending { position, slot, quality, improves, .. } = pending;
            match (improves, done.region) {
                (Some(index), Some(region)) => self.replace(index, region),
                (None, region) if self.table[position][slot] == PENDING => {
                    self.install(position, slot, region, quality);
                }
                _ => {}
            }
        }
    }

    /// Compiles a region on the thread of the hart.
    fn compile_inline(&mut self, quality: Quality, request: &Request<'_>) -> Option<Region> {
        let jit = self.inline[quality as usize].get_or_insert_with(|| Jit::new(quality));
        let start = Instant::now();
        let region = jit.as_mut()?.compile(request);
        self.stats.compile_time += start.elapsed();
        region
    }
}

/// Returns the instruction in a slot, if it is decoded and lies within the page.
fn instruction(code: &CodePage, slot: usize) -> Option<Instruction> {
    let op = code[slot];
    if op.code() < code::FIRST_OPCODE {
        return None;
    }
    let instruction = op.instruction();
    // An instruction that starts in the last slot ends in the next page.
    (slot + instruction.length as usize / 2 <= SLOTS).then_some(instruction)
}

impl Cpu {
    /// Sets how the hart uses the JIT compiler, or turns the JIT off with None. A hart starts
    /// with the default [`JitConfig`].
    pub fn set_jit(&mut self, config: Option<JitConfig>) {
        self.jit.set_config(config);
    }

    /// Returns how the hart uses the JIT compiler, or None if it does not.
    pub const fn jit(&self) -> Option<JitConfig> {
        self.jit.config()
    }

    /// Returns what the JIT has done so far.
    pub const fn jit_stats(&self) -> JitStats {
        self.jit.stats()
    }

    /// Asks for the compilation of the region that starts at slot of the page at position, or, if
    /// improves names a region, for a better version of that region.
    fn compile_region(
        &mut self,
        page: PageIndex,
        position: usize,
        slot: usize,
        improves: Option<usize>,
    ) {
        let Some(config) = self.jit.config() else { return };
        let code = &self.code.pages[position];
        let quality = match improves {
            Some(_) => Quality::Fast,
            None => JitState::first_quality(config),
        };
        match config.compilation {
            Compilation::Inline => {
                let region = self.jit.compile_inline(
                    quality,
                    &Request {
                        base: page_base(page),
                        header: slot,
                        max_instructions: config.max_region_instructions,
                        instruction: &|slot| instruction(code, slot),
                        weights: self.weights.as_ref().map(|weights| &weights.by_opcode()[..]),
                    },
                );
                match (improves, region) {
                    (Some(index), Some(region)) => self.jit.replace(index, region),
                    (Some(_), None) => {}
                    (None, region) => self.jit.install(position, slot, region, quality),
                }
            }
            Compilation::Background => {
                let worker = self.jit.worker.get_or_insert_with(Background::new);
                let id = self.jit.next_job;
                self.jit.next_job += 1;
                worker.submit(Job {
                    id,
                    quality,
                    base: page_base(page),
                    header: slot,
                    max_instructions: config.max_region_instructions,
                    instructions: (0..SLOTS).map(|slot| instruction(code, slot)).collect(),
                    weights: self.weights.as_ref().map(|weights| weights.by_opcode().clone()),
                });
                self.jit.pending.insert(
                    id,
                    Pending {
                        position,
                        slot,
                        quality,
                        generation: self.jit.generations[position],
                        improves,
                    },
                );
                if improves.is_none() {
                    self.jit.table[position][slot] = PENDING;
                }
            }
        }
    }

    /// Runs the region that can be entered at slot of the page at position, if there is one and
    /// the budget in remaining allows it. Has the region compiled the first time.
    pub(super) fn enter_region<M: Memory>(
        &mut self,
        memory: &mut M,
        page: PageIndex,
        position: usize,
        slot: usize,
        remaining: &mut u64,
        retired: &mut u64,
    ) {
        let threshold = self.jit.threshold();
        // Regions access memory through the software TLB that the memory describes.
        let (true, Some(probe)) = (self.jit.enabled(), memory.probe()) else {
            self.code.counters[position][slot] = 0;
            return;
        };

        self.jit.ensure_page(position);
        self.jit.collect();
        if self.jit.table[position][slot] == UNKNOWN {
            self.compile_region(page, position, slot, None);
        }
        let entry = self.jit.table[position][slot];
        if entry < COMPILED {
            // Look again later: the region is not ready, or cannot be compiled, or the page has
            // changed.
            self.code.counters[position][slot] = 0;
            return;
        }
        let index = (entry - COMPILED) as usize;
        let compiled = &self.jit.regions[index];
        let region = &compiled.region;
        // Make the next trip around any of the loops of the region come back here.
        let counters = &mut self.code.counters[position];
        for entry in region.entries().iter() {
            counters[*entry] = threshold - 1;
        }
        if *remaining < region.max_cost() {
            return;
        }
        let entry_index = region.entries().binary_search(&slot).expect("the table maps entries");

        let budget = match compiled.quality {
            Quality::Quick => (*remaining).min(QUICK_QUANTUM).max(region.max_cost()),
            Quality::Fast => *remaining,
        };
        let mut ctx = Ctx { budget: budget as i64, retired: 0, spent: 0, pc: 0, probe };
        // SAFETY: The region was compiled for this register file and for a software TLB that
        // follows the contract of [brisc_jit::Probe], and the budget is at least what the region
        // may spend before it checks it.
        unsafe { (region.entry())(self.registers.0.as_mut_ptr(), &mut ctx, entry_index as u64) };
        *remaining -= ctx.spent;
        *retired += ctx.retired;
        self.pc = ctx.pc;

        self.jit.stats.entries += 1;
        self.jit.stats.retired += ctx.retired;
        let limit = self.jit.config().and_then(|config| config.optimize_after);
        let compiled = &mut self.jit.regions[index];
        compiled.retired += ctx.retired;
        if let Some(limit) = limit {
            if compiled.quality == Quality::Quick &&
                !compiled.improving &&
                compiled.retired >= limit
            {
                compiled.improving = true;
                let header = compiled.header;
                self.compile_region(page, position, header, Some(index));
            }
        }
    }
}

// An [Op] is an instruction packed into a word.
const _: () = assert!(size_of::<Op>() == 8);

#[cfg(test)]
mod test {
    //! Checks that compiled code does exactly what the interpreter does.
    //!
    //! Every test runs a program twice, on the interpreter alone and with the JIT compiling loops
    //! as soon as they repeat, and compares the two after every slice of the budget: the stop,
    //! the registers, the program counter, the number of instructions retired, what they cost
    //! and the memory. The memory charges for growing, so what the stores to new pages cost is
    //! compared too.

    use super::*;
    use crate::{
        cpu::{Stop, Weights},
        memory::{Memory, SparseMemory},
    };
    use brisc_isa::{rv, rv_code, Format, Instruction, Opcode, XWord, REG_A0};

    /// A memory whose growth is priced.
    type Mem = SparseMemory<Box<dyn Fn(usize, usize) -> u64>>;

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
        spent: u64,
        memory: Vec<u8>,
        pages: usize,
    }

    /// A configuration that compiles inline as soon as a loop repeats, and optimizes after the
    /// given number of instructions, if at all.
    const fn inline(optimize_after: Option<u64>) -> JitConfig {
        JitConfig {
            threshold: 2,
            compilation: Compilation::Inline,
            max_region_instructions: 1500,
            optimize_after,
        }
    }

    fn machine(
        code: &[u8],
        registers: &[XWord; 32],
        jit: Option<JitConfig>,
        weights: Option<Weights>,
    ) -> (Cpu, Mem) {
        // The price grows faster than linearly in the number of pages, as the price of memory may.
        let growth: Box<dyn Fn(usize, usize) -> u64> =
            Box::new(|before, after| 3 * (after * after - before * before) as u64);
        let mut memory = SparseMemory::new().with_growth(growth);
        memory.write(CODE, code).unwrap();
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let data: Vec<u8> = (0..DATA_SIZE).map(|_| rng.next() as u8).collect();
        memory.write(DATA, &data).unwrap();
        let mut cpu = Cpu::new(CODE);
        cpu.set_jit(jit);
        cpu.set_weights(weights);
        for (i, value) in registers.iter().enumerate() {
            cpu.registers.set(i, *value);
        }
        (cpu, memory)
    }

    fn snapshot(cpu: &Cpu, memory: &mut Mem, stop: Stop) -> Snapshot {
        let mut image = vec![0; DATA_SIZE + CODE_SIZE];
        memory.read(DATA, &mut image[..DATA_SIZE]).unwrap();
        memory.read(CODE, &mut image[DATA_SIZE..]).unwrap();
        Snapshot {
            stop,
            registers: core::array::from_fn(|i| cpu.registers.get(i)),
            pc: cpu.pc,
            instret: cpu.instret,
            spent: cpu.spent,
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

    /// Runs `code` on both engines in slices of the given sizes, until `total` units have been
    /// spent or the program stops, and reports what happened.
    fn compare(
        code: &[u8],
        registers: &[XWord; 32],
        slices: &[u64],
        total: u64,
        what: &str,
        config: JitConfig,
    ) -> Outcome {
        compare_weighted(code, registers, slices, total, what, config, None)
    }

    /// Like [`compare`], with weights for the opcodes.
    fn compare_weighted(
        code: &[u8],
        registers: &[XWord; 32],
        slices: &[u64],
        total: u64,
        what: &str,
        config: JitConfig,
        weights: Option<Weights>,
    ) -> Outcome {
        let (mut plain, mut plain_memory) = machine(code, registers, None, weights.clone());
        let (mut jitted, mut jitted_memory) = machine(code, registers, Some(config), weights);
        let mut spent = 0;
        for slice in slices.iter().cycle() {
            let budget = (*slice).min(total - spent);
            let before = spent;
            let a = plain.run(&mut plain_memory, budget);
            let b = jitted.run(&mut jitted_memory, budget);
            let a = snapshot(&plain, &mut plain_memory, a);
            let b = snapshot(&jitted, &mut jitted_memory, b);
            assert_eq!(a, b, "{what}: after {} instructions", plain.instret);
            spent = plain.spent;
            // What remains may not cover the next instruction, and then nothing more runs.
            let stalled = a.stop == Stop::Budget && spent == before && budget == total - before;
            if a.stop != Stop::Budget || spent >= total || stalled {
                return Outcome {
                    compiled: jitted.jit_stats().retired,
                    retired: plain.instret,
                    stop: a.stop,
                };
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
        let (mut cpu, mut memory) = machine(&code, &[0; 32], Some(inline(None)), None);
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

    #[test]
    fn compiled_regions_spend_by_the_weights_in_force() {
        let code = rv_code! {
            addi a0, zero, 0;
            addi a1, zero, 1000;
            add a0, a0, a1;
            addi a1, a1, -1;
            bne a1, zero, -8;
            ecall;
        };
        let by_opcode = |scale: u32| Weights::new(move |opcode| scale * (1 + opcode as u32 % 3));
        let (mut plain, mut plain_memory) = machine(&code, &[0; 32], None, Some(by_opcode(1)));
        let (mut jitted, mut jitted_memory) =
            machine(&code, &[0; 32], Some(inline(None)), Some(by_opcode(1)));
        for scale in [1, 4, 2] {
            // The regions compiled under the previous weights must not run under these.
            plain.set_weights(Some(by_opcode(scale)));
            jitted.set_weights(Some(by_opcode(scale)));
            let a = plain.run(&mut plain_memory, 3000);
            let b = jitted.run(&mut jitted_memory, 3000);
            let a = snapshot(&plain, &mut plain_memory, a);
            let b = snapshot(&jitted, &mut jitted_memory, b);
            assert_eq!(a, b, "weights scaled by {scale}");
        }
        assert!(jitted.jit_stats().retired > 0, "some of the program ran in compiled code");
    }

    /// The largest weight that the tests give to an opcode.
    const MAX_WEIGHT: u64 = 9;

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
                    // The last three are past the memory that the machine starts with, so stores
                    // there map pages, which the memory charges for.
                    let offsets = [0x800, 0xffe, 0x1000, 0x1ff8, 0x2002, 0x3000 - 3];
                    let past = DATA_SIZE as XWord;
                    let offsets =
                        [offsets[rng.below(6) as usize], past + 0x10, past + 0xffe, past + 0x2ff8];
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
            // Every other program weighs its instructions, and runs for as many units as the others
            // run instructions.
            let weights = (seed % 2 == 0).then(|| {
                Weights::new(|opcode| {
                    let mut weight =
                        Rng(seed ^ (opcode as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                    1 + weight.below(MAX_WEIGHT) as u32
                })
            });
            let outcome = compare_weighted(
                &code,
                &registers,
                &slices,
                6000,
                &format!("seed {seed}"),
                config,
                weights,
            );
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
}
