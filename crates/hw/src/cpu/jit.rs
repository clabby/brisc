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
        for slot in region.entries.iter() {
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
        for slot in old.entries.iter() {
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
        for entry in region.entries.iter() {
            counters[*entry] = threshold - 1;
        }
        if *remaining < region.max_cost {
            return;
        }
        let entry_index = region.entries.binary_search(&slot).expect("the table maps entries");

        let budget = match compiled.quality {
            Quality::Quick => (*remaining).min(QUICK_QUANTUM).max(region.max_cost),
            Quality::Fast => *remaining,
        };
        let mut ctx =
            Ctx { budget: budget.min(i64::MAX as u64) as i64, retired: 0, spent: 0, pc: 0, probe };
        // SAFETY: The region was compiled for this register file and for a software TLB that
        // follows the contract of [brisc_jit::Probe], and the budget is at least what the region
        // may spend before it checks it.
        unsafe { (region.entry)(self.registers.0.as_mut_ptr(), &mut ctx, entry_index as u64) };
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
