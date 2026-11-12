//! A Cranelift JIT compiler for hot regions of RISC-V programs.
//!
//! This crate is an experiment. The interpreter in `brisc-hw` decodes a page of guest code into
//! slots, one for every two bytes of the page. When control keeps flowing back to a slot, the
//! interpreter hands this crate a [Request]: the page, the slot, and a way to look at the decoded
//! instructions of the page. [Jit::compile] follows the control flow from that slot to find a
//! _region_ of the page, translates it to machine code and returns it as a [Region]. A
//! [Background] does the same on a thread of its own.
//!
//! # Regions
//!
//! A region is a set of instructions of one page that are reachable from its first instruction
//! without leaving the page, together with the control flow between them. Its control flow can
//! contain loops, and inside a region the guest registers are values of the generated code, so a
//! loop of arithmetic runs without any access to the register file in memory.
//!
//! A region is entered at its first instruction or at an instruction that a jump back goes to,
//! which are the places that an interpreter that has left the region returns to: [Region::entries]
//! lists them.
//!
//! Calls and returns do not end a region. A jump through a register that the instruction before
//! it set with `auipc`, which is how a call to a function in the same binary is written, has a
//! known target and is followed. A jump through a register that holds a return address is compared
//! with the addresses after the calls in the region, up to a few, and stays in the region if one
//! matches.
//!
//! The region stops at every instruction that it does not translate (system calls, breakpoints,
//! control and status registers and atomics), at other indirect jumps, at instructions that are not
//! decoded yet and at control flow that leaves the page. The interpreter continues there.
//!
//! # Running a region
//!
//! A region is a function that takes the register file and a [Ctx]. It runs until it leaves the
//! region, writes the registers that it changed back to the register file, and records in the [Ctx]
//! the number of instructions that it retired and the program counter at which the interpreter
//! continues. The instruction at that program counter has not been executed.
//!
//! To retire exactly the instructions that the interpreter would, a region must only be entered
//! with a budget of at least [Region::max_retire] instructions. A region can only run more than
//! that many instructions by flowing back to an earlier instruction, so it checks the budget at
//! each such jump and leaves for the interpreter if fewer than [Region::max_retire] instructions
//! remain.
//!
//! # Memory
//!
//! Loads and stores are done in the generated code through the software TLB that a [Probe]
//! describes. An access that the software TLB does not serve is not done by the region: the region
//! leaves before the instruction, and the interpreter executes it, with all the cases that it
//! handles: unmapped and watched pages, accesses across pages, and errors of the memory. A region
//! therefore contains no calls and needs no way to report errors.
//!
//! # Exactness
//!
//! Whatever the regions do, the registers, the program counter and the number of instructions
//! retired are those of the interpreter whenever control is in the interpreter, which the tests of
//! `brisc-hw` check by running programs on both and comparing them.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("brisc-jit generates code for hosts with 64-bit pointers");

use brisc_isa::{Instruction, Opcode};

use core::mem::offset_of;
use cranelift_codegen::{
    ir::{condcodes::IntCC, types, AbiParam, Block, BlockArg, InstBuilder, MemFlagsData, Value},
    settings::{self, Configurable},
    Context,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use std::collections::{BTreeMap, BTreeSet};

/// The number of slots in a page of decoded code: one for every two bytes of the page.
pub const SLOTS: usize = 2048;

/// The number of entries in the software TLB.
pub const TLB_ENTRIES: u64 = 1024;

/// The size in bytes of an entry of the software TLB.
pub const TLB_ENTRY_SIZE: i64 = 24;

/// The offset in an entry of the software TLB of the page that the entry lets loads access, or of
/// a tag that matches no page.
pub const TLB_READ_TAG: i32 = 0;

/// The offset in an entry of the software TLB of the page that the entry lets stores access, or of
/// a tag that matches no page.
pub const TLB_WRITE_TAG: i32 = 8;

/// The offset in an entry of the software TLB of the number of the frame that holds the page.
pub const TLB_FRAME: i32 = 16;

/// The size in bytes of a frame, which is also the size of a page.
pub const FRAME_SIZE: i64 = 4096;

/// How much effort the compiler puts into the code of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Compile as fast as possible. The code runs about a third slower than [Quality::Fast]'s
    /// and takes about a third of the time to generate.
    Quick,
    /// Optimize, which takes several times as long as [Quality::Quick].
    Fast,
}

/// A software TLB that generated code can probe to access memory without a call.
///
/// The entry for a page is at index page % TLB_ENTRIES of the array that the probe's `tlb`
/// points to. An entry is [TLB_ENTRY_SIZE] bytes, with the three fields at the offsets above. If
/// the tag for the kind of access is the index of the page and the access does not cross the end
/// of the page, the bytes of the access are at the offset within the page in the frame that
/// [TLB_FRAME] names; frame n begins [FRAME_SIZE] times n bytes after the probe's `frames`.
///
/// Generated code reads and writes memory through the probe without asking the memory, so a probe
/// can only describe a software TLB that serves exactly the accesses that the memory would perform
/// the same way: with no error and no side effect, and without letting a store reach a page
/// that the memory has to hear about. That is why a probe can only be made with the unsafe
/// [Probe::new].
///
/// Making a probe without `unsafe` does not compile:
///
/// ```compile_fail
/// let _ = brisc_jit::Probe::new(core::ptr::null(), core::ptr::null_mut());
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    /// The first entry of the software TLB.
    tlb: *const u8,
    /// The first byte of the first frame.
    frames: *const u8,
}

impl Probe {
    /// Describes the software TLB whose first entry is at tlb and whose frames begin at frames.
    ///
    /// # Safety
    ///
    /// - tlb must point to [TLB_ENTRIES] entries laid out as described above, and frames to the
    ///   frames that they name, as long as the probe is used. The owner of the memory makes a new
    ///   probe whenever the frames move.
    /// - A hit in the software TLB must be an access that the memory would perform in the same way,
    ///   as described above. A page that the memory has to hear about writes to must have no write
    ///   tag.
    pub const unsafe fn new(tlb: *const u8, frames: *mut u8) -> Self {
        Self { tlb, frames: frames as *const u8 }
    }
}

// SAFETY: A probe only holds addresses. Whoever dereferences them has to ensure that this is
// sound, which compiled code does while it runs on the thread that owns the memory.
unsafe impl Send for Probe {}
// SAFETY: See above.
unsafe impl Sync for Probe {}

/// The context that a region runs in.
#[repr(C)]
#[derive(Debug)]
pub struct Ctx {
    /// The number of instructions that the region may retire, when it is entered.
    pub budget: i64,
    /// The number of instructions that the region retired, when it exits.
    pub retired: u64,
    /// The program counter at which the interpreter continues, when the region exits.
    pub pc: u64,
    /// The software TLB that the region probes.
    pub probe: *const Probe,
}

/// The signature of a compiled region.
pub type Entry = unsafe extern "C" fn(regs: *mut u64, ctx: *mut Ctx, entry: u64);

/// What the interpreter asks the compiler to translate.
#[allow(missing_debug_implementations)]
pub struct Request<'a> {
    /// The address of the page.
    pub base: u64,
    /// The slot at which the region starts.
    pub header: usize,
    /// The largest number of instructions in the region, at least one. Control flow beyond that is
    /// left to the interpreter.
    pub max_instructions: usize,
    /// Returns the instruction in a slot of the page, if it has been decoded and does not extend
    /// beyond the page.
    pub instruction: &'a dyn Fn(usize) -> Option<Instruction>,
}

/// A compiled region.
///
/// The code lives as long as the [Jit] that compiled it. A [Region] must not be run after that
/// [Jit] is dropped.
#[derive(Clone, Debug)]
pub struct Region {
    /// The code. It takes the register file, a context and the index in [Region::entries] of the
    /// instruction to start at.
    pub entry: Entry,
    /// The slots at which the region can be entered, in ascending order: its first instruction
    /// and the instructions that jumps back go to.
    pub entries: Box<[usize]>,
    /// The number of instructions that must remain in the budget to enter the region.
    pub max_retire: u64,
    /// The number of instructions in the region.
    pub instructions: usize,
}

/// Compiles regions for the host.
///
/// The code of the regions is released when the [Jit] is dropped, not before: a region that is
/// replaced or no longer wanted keeps its code until then.
#[allow(missing_debug_implementations)]
pub struct Jit {
    /// The module owns the memory that the code of the regions is in. It is only taken out to
    /// release that memory.
    module: core::mem::ManuallyDrop<JITModule>,
    builder_ctx: FunctionBuilderContext,
    count: usize,
}

impl Jit {
    /// Creates a compiler for the host, or returns None if the host is not supported.
    pub fn new(quality: Quality) -> Option<Self> {
        let mut flags = settings::builder();
        let opt_level = match quality {
            Quality::Quick => "none",
            Quality::Fast => "speed",
        };
        flags.set("opt_level", opt_level).ok()?;
        flags.set("use_colocated_libcalls", "false").ok()?;
        flags.set("is_pic", "false").ok()?;
        flags.set("enable_verifier", "false").ok()?;
        let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
        Some(Self {
            module: core::mem::ManuallyDrop::new(JITModule::new(JITBuilder::with_isa(
                isa,
                default_libcall_names(),
            ))),
            builder_ctx: FunctionBuilderContext::new(),
            count: 0,
        })
    }

    /// Translates the region that [Request] describes, or returns None if the instruction at
    /// its header is not one that regions contain.
    pub fn compile(&mut self, request: &Request<'_>) -> Option<Region> {
        let shape = Shape::discover(request)?;
        debug_assert!(shape.entries.contains(&request.header));

        let mut ctx = Context::new();
        let pointer = self.module.target_config().pointer_type();
        ctx.func.signature.params.push(AbiParam::new(pointer));
        ctx.func.signature.params.push(AbiParam::new(pointer));
        ctx.func.signature.params.push(AbiParam::new(types::I64));
        let frontend_config = self.module.target_config();
        let mut lowering = Lowering::new(
            FunctionBuilder::new(&mut ctx.func, &mut self.builder_ctx),
            request,
            &shape,
        );
        lowering.lower();
        lowering.builder.finalize(frontend_config);

        self.count += 1;
        let name = format!("region_{}", self.count);
        let id = self.module.declare_function(&name, Linkage::Local, &ctx.func.signature).ok()?;
        self.module.define_function(id, &mut ctx).ok()?;
        self.module.finalize_definitions().ok()?;
        let code = self.module.get_finalized_function(id);
        self.module.clear_context(&mut ctx);

        Some(Region {
            // SAFETY: The code was generated for the signature of [Entry].
            entry: unsafe { core::mem::transmute::<*const u8, Entry>(code) },
            entries: shape.entries.into_boxed_slice(),
            max_retire: shape.instructions.len() as u64,
            instructions: shape.instructions.len(),
        })
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        // SAFETY: The module is not used again, and by the contract of [Region] no region that
        // it compiled runs any more.
        unsafe { core::mem::ManuallyDrop::take(&mut self.module).free_memory() };
    }
}

/// A request for a compiler thread, which owns what it needs.
#[derive(Debug)]
pub struct Job {
    /// Identifies the job in its [Done].
    pub id: u64,
    /// The quality of the code.
    pub quality: Quality,
    /// The address of the page.
    pub base: u64,
    /// The slot at which the region starts.
    pub header: usize,
    /// See [Request::max_instructions].
    pub max_instructions: usize,
    /// The instruction in each slot of the page, as for [Request::instruction].
    pub instructions: Vec<Option<Instruction>>,
}

/// The result of a [Job].
#[derive(Debug, Clone)]
pub struct Done {
    /// The [Job::id].
    pub id: u64,
    /// The region, or None if no region can start at the header.
    pub region: Option<Region>,
    /// The time that compiling took.
    pub compile_time: std::time::Duration,
}

/// A thread that compiles [Job]s while the program keeps running.
///
/// The thread has a [Jit] for each [Quality], and the regions that it compiles are valid until the
/// [Background] is dropped, which lets the thread end and release their code. A thread that is
/// in the middle of a job finishes that job first.
#[derive(Debug)]
pub struct Background {
    jobs: std::sync::mpsc::Sender<Job>,
    done: std::sync::mpsc::Receiver<Done>,
}

impl Default for Background {
    fn default() -> Self {
        Self::new()
    }
}

impl Background {
    /// Starts the thread.
    pub fn new() -> Self {
        let (jobs, job_queue) = std::sync::mpsc::channel::<Job>();
        let (done_sender, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut compilers: [Option<Option<Jit>>; 2] = [None, None];
            for job in job_queue {
                let start = std::time::Instant::now();
                let compiler = &mut compilers[job.quality as usize];
                let jit = compiler.get_or_insert_with(|| Jit::new(job.quality));
                let compiled = jit.as_mut().map(|jit| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        jit.compile(&Request {
                            base: job.base,
                            header: job.header,
                            max_instructions: job.max_instructions,
                            instruction: &|slot| job.instructions[slot],
                        })
                    }))
                });
                let region = match compiled {
                    Some(Ok(region)) => region,
                    Some(Err(_)) => {
                        // The compiler is not trusted again. It is not dropped either, because
                        // that would release the code of the regions that it compiled before,
                        // which may be running.
                        core::mem::forget(compiler.take());
                        None
                    }
                    None => None,
                };
                let done = Done { id: job.id, region, compile_time: start.elapsed() };
                if done_sender.send(done).is_err() {
                    break;
                }
            }
        });
        Self { jobs, done }
    }

    /// Queues a job.
    pub fn submit(&self, job: Job) {
        // The thread only ends after the sender is dropped.
        let _ = self.jobs.send(job);
    }

    /// Returns the jobs that have finished since the last call.
    pub fn finished(&self) -> impl Iterator<Item = Done> + '_ {
        self.done.try_iter()
    }
}

/// Returns whether regions contain `opcode`.
const fn supported(opcode: Opcode) -> bool {
    use Opcode::*;
    matches!(
        opcode,
        Lui | Auipc |
            Jal |
            Jalr |
            Beq |
            Bne |
            Blt |
            Bge |
            Bltu |
            Bgeu |
            Lb |
            Lh |
            Lw |
            Lbu |
            Lhu |
            Lwu |
            Ld |
            Sb |
            Sh |
            Sw |
            Sd |
            Addi |
            Slti |
            Sltiu |
            Xori |
            Ori |
            Andi |
            Slli |
            Srli |
            Srai |
            Add |
            Sub |
            Sll |
            Slt |
            Sltu |
            Xor |
            Srl |
            Sra |
            Or |
            And |
            Addiw |
            Slliw |
            Srliw |
            Sraiw |
            Addw |
            Subw |
            Sllw |
            Srlw |
            Sraw |
            Mul |
            Mulh |
            Mulhsu |
            Mulhu |
            Div |
            Divu |
            Rem |
            Remu |
            Mulw |
            Divw |
            Divuw |
            Remw |
            Remuw |
            Fence |
            FenceI
    )
}

/// The instructions of a region and where control flow within it begins.
struct Shape {
    /// The instructions by slot.
    instructions: BTreeMap<usize, Instruction>,
    /// The slots at which a basic block begins.
    leaders: BTreeSet<usize>,
    /// The slots after the jumps that link, which indirect jumps may return to.
    return_sites: Vec<usize>,
    /// The indirect jumps whose target is known because the instruction before them computed it
    /// with `auipc`, by slot, with the slot of the target in the page.
    resolved: BTreeMap<usize, usize>,
    /// The slots at which a region can be entered, in ascending order: the first instruction and
    /// every instruction that a jump back goes to.
    entries: Vec<usize>,
}

/// The largest number of return sites that an indirect jump is compared with.
const MAX_RETURN_SITES: usize = 8;

impl Shape {
    /// Follows control flow from the header of `request`.
    fn discover(request: &Request<'_>) -> Option<Self> {
        if request.max_instructions == 0 {
            return None;
        }
        let mut instructions = BTreeMap::new();
        let mut leaders = BTreeSet::new();
        let mut return_sites = BTreeSet::new();
        // The indirect jumps with a known target: the slot of the target, and the slot of the first
        // instruction that the target was computed from.
        let mut resolved: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        let mut work = vec![request.header];
        leaders.insert(request.header);

        let region_instruction = |slot: usize| {
            (slot < SLOTS)
                .then(|| (request.instruction)(slot))
                .flatten()
                .filter(|instruction| supported(instruction.opcode))
        };
        region_instruction(request.header)?;

        while let Some(mut slot) = work.pop() {
            // The registers that are known to hold a constant, as far as the straight-line code
            // from the start of this walk shows, with the slot of the first instruction that the
            // value was computed from.
            let mut constants: BTreeMap<u8, (u64, usize)> = BTreeMap::new();
            loop {
                if instructions.contains_key(&slot) {
                    // Another walk has been here, so control can enter here from two places.
                    leaders.insert(slot);
                    break;
                }
                let Some(instruction) = region_instruction(slot) else { break };
                if instructions.len() == request.max_instructions {
                    break;
                }
                instructions.insert(slot, instruction);

                let pc = request.base.wrapping_add(2 * slot as u64);
                let next = slot + instruction.length as usize / 2;
                let known = |register: u8, constants: &BTreeMap<u8, (u64, usize)>| {
                    if register == 0 {
                        Some((0, slot))
                    } else {
                        constants.get(&register).copied()
                    }
                };
                let target = slot as i64 + (instruction.imm as i64 >> 1);
                let before = constants.clone();
                match instruction.opcode {
                    Opcode::Auipc => {
                        constants.insert(
                            instruction.rd,
                            (pc.wrapping_add(instruction.imm as i64 as u64), slot),
                        );
                    }
                    Opcode::Lui => {
                        constants.insert(instruction.rd, (instruction.imm as i64 as u64, slot));
                    }
                    Opcode::Addi => match known(instruction.rs1, &before) {
                        Some((value, origin)) => {
                            constants.insert(
                                instruction.rd,
                                (value.wrapping_add(instruction.imm as i64 as u64), origin),
                            );
                        }
                        None => {
                            constants.remove(&instruction.rd);
                        }
                    },
                    opcode if writes_register(opcode) => {
                        constants.remove(&instruction.rd);
                    }
                    _ => {}
                }
                match instruction.opcode {
                    Opcode::Jal => {
                        if (0..SLOTS as i64).contains(&target) && leaders.insert(target as usize) {
                            work.push(target as usize);
                        }
                        // The callee returns to the instruction after the call.
                        if instruction.rd != 0 && next < SLOTS {
                            return_sites.insert(next);
                            if leaders.insert(next) {
                                work.push(next);
                            }
                        }
                        break;
                    }
                    Opcode::Jalr => {
                        // A call through a register that `auipc` has just loaded has a known
                        // target.
                        if let Some((base, origin)) = known(instruction.rs1, &before) {
                            let target_pc = base.wrapping_add(instruction.imm as i64 as u64) & !1;
                            let offset = target_pc.wrapping_sub(request.base);
                            if offset < 2 * SLOTS as u64 {
                                let target = (offset / 2) as usize;
                                resolved.insert(slot, (target, origin));
                                if leaders.insert(target) {
                                    work.push(target);
                                }
                            }
                        }
                        if instruction.rd != 0 && next < SLOTS {
                            return_sites.insert(next);
                            if leaders.insert(next) {
                                work.push(next);
                            }
                        }
                        break;
                    }
                    Opcode::Beq |
                    Opcode::Bne |
                    Opcode::Blt |
                    Opcode::Bge |
                    Opcode::Bltu |
                    Opcode::Bgeu => {
                        if (0..SLOTS as i64).contains(&target) && leaders.insert(target as usize) {
                            work.push(target as usize);
                        }
                        leaders.insert(next);
                        slot = next;
                    }
                    _ => slot = next,
                }
            }
        }
        // The registers are only known if control cannot enter between the instructions that
        // computed the target and the jump, which it can where a basic block begins.
        let resolved: BTreeMap<usize, usize> = resolved
            .into_iter()
            .filter(|(slot, (_, origin))| leaders.range(origin + 1..=*slot).next().is_none())
            .map(|(slot, (target, _))| (slot, target))
            .collect();
        let mut entries = BTreeSet::from([request.header]);
        for (slot, instruction) in &instructions {
            let target = match instruction.opcode {
                Opcode::Jal |
                Opcode::Beq |
                Opcode::Bne |
                Opcode::Blt |
                Opcode::Bge |
                Opcode::Bltu |
                Opcode::Bgeu => usize::try_from(*slot as i64 + (instruction.imm as i64 >> 1)).ok(),
                Opcode::Jalr => resolved.get(slot).copied(),
                _ => None,
            };
            if let Some(target) = target.filter(|target| target <= slot) {
                if instructions.contains_key(&target) {
                    entries.insert(target);
                }
            }
        }
        Some(Self {
            instructions,
            leaders,
            return_sites: return_sites.into_iter().collect(),
            resolved,
            entries: entries.into_iter().collect(),
        })
    }
}

/// Generates the code of a region.
struct Lowering<'a, 'b> {
    builder: FunctionBuilder<'b>,
    request: &'a Request<'a>,
    shape: &'a Shape,
    /// The guest registers `x1` to `x31`, at indices 1 to 31.
    vars: [Variable; 32],
    /// The number of instructions retired so far.
    retired: Variable,
    regs: Value,
    ctx: Value,
    /// The value of [Ctx::budget] minus the number of instructions of the region.
    limit: Value,
    /// The software TLB, if the region probes it.
    tlb: Option<Value>,
    probe: Option<Value>,
    /// The block that writes back the registers and returns. Its parameters are the program
    /// counter and the exit code.
    epilogue: Block,
    /// The registers that the region writes.
    written: Vec<u8>,
    blocks: BTreeMap<usize, Block>,
    /// The edges that have to be generated once the block that needs them is complete.
    edges: Vec<Edge>,
}

/// A transfer of control that needs a block of its own.
struct Edge {
    block: Block,
    /// Where the block goes if the budget allows it, or `None` if it leaves the region.
    go: Option<Block>,
    /// The program counter that the interpreter continues at when the region ends here.
    pc: u64,
}

const FLAGS: MemFlagsData = MemFlagsData::new();

impl<'a, 'b> Lowering<'a, 'b> {
    fn new(mut builder: FunctionBuilder<'b>, request: &'a Request<'a>, shape: &'a Shape) -> Self {
        let vars = core::array::from_fn(|_| builder.declare_var(types::I64));
        let retired = builder.declare_var(types::I64);
        let epilogue = builder.create_block();
        let mut written: Vec<u8> = shape
            .instructions
            .values()
            .filter(|i| writes_register(i.opcode))
            .map(|i| i.rd)
            .filter(|rd| *rd != 0)
            .collect();
        written.sort_unstable();
        written.dedup();

        Self {
            builder,
            request,
            shape,
            vars,
            retired,
            regs: Value::from_u32(0),
            ctx: Value::from_u32(0),
            limit: Value::from_u32(0),
            tlb: None,
            probe: None,
            epilogue,
            written,
            blocks: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    const fn pc(&self, slot: usize) -> u64 {
        self.request.base.wrapping_add(2 * slot as u64)
    }

    fn iconst(&mut self, value: i64) -> Value {
        self.builder.ins().iconst(types::I64, value)
    }

    fn reg(&mut self, register: u8) -> Value {
        if register == 0 {
            self.iconst(0)
        } else {
            self.builder.use_var(self.vars[register as usize])
        }
    }

    fn set(&mut self, register: u8, value: Value) {
        if register != 0 {
            self.builder.def_var(self.vars[register as usize], value);
        }
    }

    /// Retires one instruction.
    fn retire(&mut self) {
        let retired = self.builder.use_var(self.retired);
        let retired = self.builder.ins().iadd_imm_s(retired, 1);
        self.builder.def_var(self.retired, retired);
    }

    /// Leaves the region at the program counter in pc.
    fn exit(&mut self, pc: Value) {
        self.builder.ins().jump(self.epilogue, &[BlockArg::Value(pc)]);
    }

    fn exit_to(&mut self, pc: u64) {
        let pc = self.iconst(pc as i64);
        self.exit(pc);
    }

    fn lower(&mut self) {
        self.blocks.clear();
        for slot in &self.shape.leaders {
            if self.shape.instructions.contains_key(slot) {
                let block = self.builder.create_block();
                self.blocks.insert(*slot, block);
            }
        }

        let entry = self.builder.create_block();
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);
        self.regs = self.builder.block_params(entry)[0];
        self.ctx = self.builder.block_params(entry)[1];

        let budget =
            self.builder.ins().load(types::I64, FLAGS, self.ctx, offset_of!(Ctx, budget) as i32);
        self.limit = self.builder.ins().iadd_imm_s(budget, -(self.shape.instructions.len() as i64));
        let probe =
            self.builder.ins().load(types::I64, FLAGS, self.ctx, offset_of!(Ctx, probe) as i32);
        let tlb = self.builder.ins().load(types::I64, FLAGS, probe, offset_of!(Probe, tlb) as i32);
        self.probe = Some(probe);
        self.tlb = Some(tlb);
        let zero = self.iconst(0);
        self.builder.def_var(self.retired, zero);

        // Registers that the region reads or writes.
        let mut used = BTreeSet::new();
        for instruction in self.shape.instructions.values() {
            for register in [instruction.rd, instruction.rs1, instruction.rs2] {
                if register != 0 {
                    used.insert(register);
                }
            }
        }
        for register in used {
            let value = self.builder.ins().load(types::I64, FLAGS, self.regs, 8 * register as i32);
            self.builder.def_var(self.vars[register as usize], value);
        }
        let header = self.blocks[&self.request.header];
        if self.shape.entries.len() == 1 {
            self.builder.ins().jump(header, &[]);
        } else {
            let entry_index = self.builder.block_params(entry)[2];
            let mut switch = Switch::new();
            for (index, slot) in self.shape.entries.iter().enumerate() {
                switch.set_entry(index as u128, self.blocks[slot]);
            }
            switch.emit(&mut self.builder, entry_index, header);
        }

        for (leader, block) in self.blocks.clone() {
            self.builder.switch_to_block(block);
            let mut slot = leader;
            loop {
                let Some(instruction) = self.shape.instructions.get(&slot).copied() else {
                    // Control flows into an instruction that the region does not contain.
                    self.exit_to(self.pc(slot));
                    break;
                };
                if slot != leader && self.blocks.contains_key(&slot) {
                    self.builder.ins().jump(self.blocks[&slot], &[]);
                    break;
                }
                if self.instruction(slot, instruction) {
                    break;
                }
                slot += instruction.length as usize / 2;
            }
        }

        self.lower_edges();

        self.builder.switch_to_block(self.epilogue);
        let pc = self.builder.append_block_param(self.epilogue, types::I64);
        for register in self.written.clone() {
            let value = self.builder.use_var(self.vars[register as usize]);
            self.builder.ins().store(FLAGS, value, self.regs, 8 * register as i32);
        }
        let retired = self.builder.use_var(self.retired);
        self.builder.ins().store(FLAGS, retired, self.ctx, offset_of!(Ctx, retired) as i32);
        self.builder.ins().store(FLAGS, pc, self.ctx, offset_of!(Ctx, pc) as i32);
        self.builder.ins().return_(&[]);
        self.builder.seal_all_blocks();
    }

    /// Returns a block that continues at `target` after the instruction at `from`, or leaves the
    /// region at `pc` if the target is not in it.
    fn edge(&mut self, from: usize, target: Option<usize>, pc: u64) -> Block {
        match target {
            Some(slot) if self.blocks.contains_key(&slot) && slot > from => self.blocks[&slot],
            Some(slot) if self.blocks.contains_key(&slot) => {
                let block = self.builder.create_block();
                self.edges.push(Edge { block, go: Some(self.blocks[&slot]), pc });
                block
            }
            _ => {
                let block = self.builder.create_block();
                self.edges.push(Edge { block, go: None, pc });
                block
            }
        }
    }

    /// Generates the blocks that [Self::edge] has handed out.
    fn lower_edges(&mut self) {
        for Edge { block, go, pc } in core::mem::take(&mut self.edges) {
            self.builder.switch_to_block(block);
            if let Some(go) = go {
                // A jump back: stop if the budget could run out before the next check.
                let stop = self.builder.create_block();
                self.builder.set_cold_block(stop);
                let retired = self.builder.use_var(self.retired);
                let over = self.builder.ins().icmp(IntCC::SignedGreaterThan, retired, self.limit);
                self.builder.ins().brif(over, stop, &[], go, &[]);
                self.builder.switch_to_block(stop);
            }
            self.exit_to(pc);
        }
    }

    /// The slot of the target of a jump or branch, if it is in the page.
    fn target(&self, slot: usize, instruction: &Instruction) -> (Option<usize>, u64) {
        let target = slot as i64 + (instruction.imm as i64 >> 1);
        let pc = self.pc(slot).wrapping_add(instruction.imm as i64 as u64);
        ((0..SLOTS as i64).contains(&target).then_some(target as usize), pc)
    }

    /// Generates an instruction. Returns whether it ends the block.
    fn instruction(&mut self, slot: usize, i: Instruction) -> bool {
        use Opcode::*;
        let imm = i.imm as i64;
        let pc = self.pc(slot);
        let next_pc = pc.wrapping_add(i.length as u64);
        match i.opcode {
            Lui => {
                let value = self.iconst(imm);
                self.set(i.rd, value);
            }
            Auipc => {
                let value = self.iconst(pc.wrapping_add(imm as u64) as i64);
                self.set(i.rd, value);
            }
            Jal => {
                let link = self.iconst(next_pc as i64);
                self.set(i.rd, link);
                self.retire();
                let (target, target_pc) = self.target(slot, &i);
                let edge = self.edge(slot, target, target_pc);
                self.builder.ins().jump(edge, &[]);
                return true;
            }
            Jalr if self.shape.resolved.contains_key(&slot) => {
                let target = self.shape.resolved[&slot];
                let target_pc = self.pc(target);
                let link = self.iconst(next_pc as i64);
                self.set(i.rd, link);
                self.retire();
                let edge = self.edge(slot, Some(target), target_pc);
                self.builder.ins().jump(edge, &[]);
                return true;
            }
            Jalr => {
                let base = self.reg(i.rs1);
                let target = self.builder.ins().iadd_imm_s(base, imm);
                let target = self.builder.ins().band_imm_s(target, !1);
                let link = self.iconst(next_pc as i64);
                self.set(i.rd, link);
                self.retire();
                // A return to a call site in the region stays in the region.
                let sites = self.shape.return_sites.clone();
                let sites: Vec<usize> = sites
                    .into_iter()
                    .filter(|site| self.blocks.contains_key(site))
                    .take(MAX_RETURN_SITES)
                    .collect();
                for site in sites {
                    let site_pc = self.pc(site);
                    let candidate = self.iconst(site_pc as i64);
                    let hit = self.builder.ins().icmp(IntCC::Equal, target, candidate);
                    let edge = self.edge(usize::MAX, Some(site), site_pc);
                    let miss = self.builder.create_block();
                    self.builder.ins().brif(hit, edge, &[], miss, &[]);
                    self.builder.switch_to_block(miss);
                }
                self.exit(target);
                return true;
            }
            Beq | Bne | Blt | Bge | Bltu | Bgeu => {
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                let cc = match i.opcode {
                    Beq => IntCC::Equal,
                    Bne => IntCC::NotEqual,
                    Blt => IntCC::SignedLessThan,
                    Bge => IntCC::SignedGreaterThanOrEqual,
                    Bltu => IntCC::UnsignedLessThan,
                    _ => IntCC::UnsignedGreaterThanOrEqual,
                };
                let condition = self.builder.ins().icmp(cc, a, b);
                self.retire();
                let (target, target_pc) = self.target(slot, &i);
                let taken = self.edge(slot, target, target_pc);
                let fallthrough_slot = slot + i.length as usize / 2;
                let fallthrough = self.edge(slot, Some(fallthrough_slot), next_pc);
                self.builder.ins().brif(condition, taken, &[], fallthrough, &[]);
                return true;
            }
            Lb | Lh | Lw | Lbu | Lhu | Lwu | Ld => {
                let base = self.reg(i.rs1);
                let address = self.builder.ins().iadd_imm_s(base, imm);
                let (bytes, signed) = match i.opcode {
                    Lb => (1, true),
                    Lh => (2, true),
                    Lw => (4, true),
                    Lbu => (1, false),
                    Lhu => (2, false),
                    Lwu => (4, false),
                    _ => (8, false),
                };
                let value = self.load(address, bytes, signed, pc);
                self.set(i.rd, value);
            }
            Sb | Sh | Sw | Sd => {
                let base = self.reg(i.rs1);
                let address = self.builder.ins().iadd_imm_s(base, imm);
                let value = self.reg(i.rs2);
                let bytes = match i.opcode {
                    Sb => 1,
                    Sh => 2,
                    Sw => 4,
                    _ => 8,
                };
                self.store(address, value, bytes, pc);
            }
            Fence | FenceI => {}
            _ => {
                if let Some(value) = self.alu(&i) {
                    self.set(i.rd, value);
                } else {
                    unreachable!("regions only contain instructions that are lowered");
                }
            }
        }
        self.retire();
        false
    }

    /// Generates a register or immediate arithmetic instruction.
    fn alu(&mut self, i: &Instruction) -> Option<Value> {
        use Opcode::*;
        let imm = i.imm as i64;
        let ins_word = |this: &mut Self,
                        f: &dyn Fn(&mut FunctionBuilder<'b>, Value, Value) -> Value,
                        immediate: bool| {
            let a = this.reg(i.rs1);
            let a = this.builder.ins().ireduce(types::I32, a);
            let b = if immediate {
                this.builder.ins().iconst(types::I32, imm)
            } else {
                let b = this.reg(i.rs2);
                this.builder.ins().ireduce(types::I32, b)
            };
            let r = f(&mut this.builder, a, b);
            this.builder.ins().sextend(types::I64, r)
        };
        Some(match i.opcode {
            Addi => {
                let a = self.reg(i.rs1);
                self.builder.ins().iadd_imm_s(a, imm)
            }
            Xori => {
                let a = self.reg(i.rs1);
                self.builder.ins().bxor_imm_s(a, imm)
            }
            Ori => {
                let a = self.reg(i.rs1);
                self.builder.ins().bor_imm_s(a, imm)
            }
            Andi => {
                let a = self.reg(i.rs1);
                self.builder.ins().band_imm_s(a, imm)
            }
            Slli => {
                let a = self.reg(i.rs1);
                self.builder.ins().ishl_imm_s(a, imm & 63)
            }
            Srli => {
                let a = self.reg(i.rs1);
                self.builder.ins().ushr_imm_s(a, imm & 63)
            }
            Srai => {
                let a = self.reg(i.rs1);
                self.builder.ins().sshr_imm_s(a, imm & 63)
            }
            Slti | Sltiu => {
                let a = self.reg(i.rs1);
                let b = self.iconst(imm);
                let cc =
                    if i.opcode == Slti { IntCC::SignedLessThan } else { IntCC::UnsignedLessThan };
                let c = self.builder.ins().icmp(cc, a, b);
                self.builder.ins().uextend(types::I64, c)
            }
            Add | Sub | Sll | Srl | Sra | Xor | Or | And | Mul => {
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                match i.opcode {
                    Add => self.builder.ins().iadd(a, b),
                    Sub => self.builder.ins().isub(a, b),
                    Sll => self.builder.ins().ishl(a, b),
                    Srl => self.builder.ins().ushr(a, b),
                    Sra => self.builder.ins().sshr(a, b),
                    Xor => self.builder.ins().bxor(a, b),
                    Or => self.builder.ins().bor(a, b),
                    And => self.builder.ins().band(a, b),
                    _ => self.builder.ins().imul(a, b),
                }
            }
            Slt | Sltu => {
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                let cc =
                    if i.opcode == Slt { IntCC::SignedLessThan } else { IntCC::UnsignedLessThan };
                let c = self.builder.ins().icmp(cc, a, b);
                self.builder.ins().uextend(types::I64, c)
            }
            Mulh => {
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                self.builder.ins().smulhi(a, b)
            }
            Mulhu => {
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                self.builder.ins().umulhi(a, b)
            }
            Mulhsu => {
                // The signed product, corrected by the first operand when the second has its sign
                // bit set and is therefore read as a large unsigned number.
                let a = self.reg(i.rs1);
                let b = self.reg(i.rs2);
                let hi = self.builder.ins().smulhi(a, b);
                let negative = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, b, 0);
                let zero = self.iconst(0);
                let correction = self.builder.ins().select(negative, a, zero);
                self.builder.ins().iadd(hi, correction)
            }
            Div | Rem | Divu | Remu => self.division(i.opcode, i.rs1, i.rs2, false),
            Divw | Remw | Divuw | Remuw => self.division(i.opcode, i.rs1, i.rs2, true),
            Addiw => ins_word(self, &|b, x, y| b.ins().iadd(x, y), true),
            Slliw => ins_word(self, &|b, x, y| b.ins().ishl(x, y), true),
            Srliw => ins_word(self, &|b, x, y| b.ins().ushr(x, y), true),
            Sraiw => ins_word(self, &|b, x, y| b.ins().sshr(x, y), true),
            Addw => ins_word(self, &|b, x, y| b.ins().iadd(x, y), false),
            Subw => ins_word(self, &|b, x, y| b.ins().isub(x, y), false),
            Sllw => ins_word(self, &|b, x, y| b.ins().ishl(x, y), false),
            Srlw => ins_word(self, &|b, x, y| b.ins().ushr(x, y), false),
            Sraw => ins_word(self, &|b, x, y| b.ins().sshr(x, y), false),
            Mulw => ins_word(self, &|b, x, y| b.ins().imul(x, y), false),
            _ => return None,
        })
    }

    /// Generates a division or remainder with the results that the M extension defines for a zero
    /// divisor and for overflow.
    fn division(&mut self, opcode: Opcode, rs1: u8, rs2: u8, word: bool) -> Value {
        use Opcode::*;
        let ty = if word { types::I32 } else { types::I64 };
        let a = self.reg(rs1);
        let b = self.reg(rs2);
        let (a, b) = if word {
            (self.builder.ins().ireduce(ty, a), self.builder.ins().ireduce(ty, b))
        } else {
            (a, b)
        };
        let signed = matches!(opcode, Div | Rem | Divw | Remw);
        let is_div = matches!(opcode, Div | Divu | Divw | Divuw);
        let zero = self.builder.ins().icmp_imm_s(IntCC::Equal, b, 0);
        let one = self.builder.ins().iconst(ty, 1);
        // A divisor that would trap is replaced by one: the signed quotient of the overflowing
        // division is then the minimum and its remainder zero, as the M extension defines, and
        // only the zero divisor needs another fix-up.
        let unsafe_divisor = if signed {
            let min = self.builder.ins().iconst(ty, if word { i32::MIN as i64 } else { i64::MIN });
            let a_min = self.builder.ins().icmp(IntCC::Equal, a, min);
            let b_neg1 = self.builder.ins().icmp_imm_s(IntCC::Equal, b, -1);
            let overflow = self.builder.ins().band(a_min, b_neg1);
            self.builder.ins().bor(zero, overflow)
        } else {
            zero
        };
        let divisor = self.builder.ins().select(unsafe_divisor, one, b);
        let result = match (signed, is_div) {
            (true, true) => self.builder.ins().sdiv(a, divisor),
            (true, false) => self.builder.ins().srem(a, divisor),
            (false, true) => self.builder.ins().udiv(a, divisor),
            (false, false) => self.builder.ins().urem(a, divisor),
        };
        let on_zero = if is_div { self.builder.ins().iconst(ty, -1) } else { a };
        let result = self.builder.ins().select(zero, on_zero, result);
        if word {
            self.builder.ins().sextend(types::I64, result)
        } else {
            result
        }
    }

    /// Generates a load and returns the value, extended to 64 bits. An access that the software
    /// TLB does not serve leaves the region before the instruction at pc, for the interpreter to
    /// execute.
    fn load(&mut self, address: Value, bytes: u32, signed: bool, pc: u64) -> Value {
        let entry = self.probe_entry(address, bytes, TLB_READ_TAG, pc);
        let ptr = self.frame_pointer(entry, address);
        match (bytes, signed) {
            (1, false) => self.builder.ins().uload8(types::I64, FLAGS, ptr, 0),
            (1, true) => self.builder.ins().sload8(types::I64, FLAGS, ptr, 0),
            (2, false) => self.builder.ins().uload16(types::I64, FLAGS, ptr, 0),
            (2, true) => self.builder.ins().sload16(types::I64, FLAGS, ptr, 0),
            (4, false) => self.builder.ins().uload32(FLAGS, ptr, 0),
            (4, true) => self.builder.ins().sload32(FLAGS, ptr, 0),
            _ => self.builder.ins().load(types::I64, FLAGS, ptr, 0),
        }
    }

    /// Generates a store, which leaves the region before the instruction at pc if the software
    /// TLB does not serve it.
    fn store(&mut self, address: Value, value: Value, bytes: u32, pc: u64) {
        let entry = self.probe_entry(address, bytes, TLB_WRITE_TAG, pc);
        let ptr = self.frame_pointer(entry, address);
        match bytes {
            1 => self.builder.ins().istore8(FLAGS, value, ptr, 0),
            2 => self.builder.ins().istore16(FLAGS, value, ptr, 0),
            4 => self.builder.ins().istore32(FLAGS, value, ptr, 0),
            _ => self.builder.ins().store(FLAGS, value, ptr, 0),
        };
    }

    /// Generates the software TLB lookup for an access of bytes bytes at address. If the access
    /// cannot be done in the region, control leaves it at pc. Returns the address of the entry,
    /// with the builder in the block of the fast path.
    fn probe_entry(&mut self, address: Value, bytes: u32, tag: i32, pc: u64) -> Value {
        let tlb = self.tlb.expect("regions probe the software TLB");
        let page = self.builder.ins().ushr_imm_s(address, 12);
        let index = self.builder.ins().band_imm_s(page, TLB_ENTRIES as i64 - 1);
        let offset = self.builder.ins().imul_imm_s(index, TLB_ENTRY_SIZE);
        let entry = self.builder.ins().iadd(tlb, offset);
        let found = self.builder.ins().load(types::I64, FLAGS, entry, tag);
        let tag_ok = self.builder.ins().icmp(IntCC::Equal, found, page);
        let within = self.builder.ins().band_imm_s(address, FRAME_SIZE - 1);
        let fits = self.builder.ins().icmp_imm_s(
            IntCC::UnsignedLessThanOrEqual,
            within,
            FRAME_SIZE - bytes as i64,
        );
        let ok = self.builder.ins().band(tag_ok, fits);
        let fast = self.builder.create_block();
        let slow = self.builder.create_block();
        self.builder.set_cold_block(slow);
        self.builder.ins().brif(ok, fast, &[], slow, &[]);
        self.builder.switch_to_block(slow);
        self.exit_to(pc);
        self.builder.switch_to_block(fast);
        entry
    }

    /// Returns the address in a frame that `address` maps to, given its TLB entry.
    fn frame_pointer(&mut self, entry: Value, address: Value) -> Value {
        let frame = self.builder.ins().load(types::I64, FLAGS, entry, TLB_FRAME);
        let probe = self.probe.expect("the region probes the TLB");
        let frames =
            self.builder.ins().load(types::I64, FLAGS, probe, offset_of!(Probe, frames) as i32);
        let base = self.builder.ins().ishl_imm_s(frame, 12);
        let base = self.builder.ins().iadd(frames, base);
        let within = self.builder.ins().band_imm_s(address, FRAME_SIZE - 1);
        self.builder.ins().iadd(base, within)
    }
}

/// Returns whether an instruction writes its destination register.
const fn writes_register(opcode: Opcode) -> bool {
    use Opcode::*;
    !matches!(opcode, Beq | Bne | Blt | Bge | Bltu | Bgeu | Sb | Sh | Sw | Sd | Fence | FenceI)
}

#[cfg(test)]
mod test {
    use super::*;
    use brisc_isa::rv;

    /// A probe for regions that have no memory instructions.
    fn unused_probe() -> Probe {
        // SAFETY: A region without memory instructions does not use the probe.
        unsafe { Probe::new(core::ptr::null(), core::ptr::null_mut()) }
    }

    const BASE: u64 = 0x1000;

    /// Compiles the instructions, given with the slots they are in, into a region that starts at
    /// the first. The slots can overlap, as the slots of a page do when instructions have
    /// different lengths and control flow goes to the middle of one. The compiler is returned too,
    /// because the code is only valid while it lives.
    fn compile_slots(program: &[(usize, Instruction)]) -> (Jit, Region) {
        let mut slots = vec![None; SLOTS];
        for (slot, instruction) in program {
            slots[*slot] = Some(*instruction);
        }
        let mut jit = Jit::new(Quality::Fast).expect("the host is supported");
        let region = jit
            .compile(&Request {
                base: BASE,
                header: program[0].0,
                max_instructions: 100,
                instruction: &|slot| slots[slot],
            })
            .expect("the program compiles");
        (jit, region)
    }

    /// Compiles the program, whose instructions are 4 bytes long and which starts at BASE.
    fn compile(program: &[u32]) -> (Jit, Region) {
        let slots: Vec<_> = program
            .iter()
            .enumerate()
            .map(|(i, word)| (2 * i, Instruction::decode(*word).unwrap()))
            .collect();
        compile_slots(&slots)
    }

    /// Compiles the loop that adds one to a0 until it equals a1, which is followed by an ecall.
    fn counting_loop() -> (Jit, Region) {
        compile(&[rv!(addi a0, a0, 1), rv!(bne a0, a1, -4), rv!(ecall)])
    }

    fn run(region: &Region, a1: u64, budget: i64) -> ([u64; 64], Ctx) {
        let mut registers = [0; 64];
        registers[11] = a1;
        registers[5] = 0x5000;
        let probe = unused_probe();
        let mut ctx = Ctx { budget, retired: 0, pc: 0, probe: &probe };
        // SAFETY: The region uses the registers and the context, and not the software TLB,
        // which it has no memory instruction to need.
        unsafe { (region.entry)(registers.as_mut_ptr(), &mut ctx, 0) };
        (registers, ctx)
    }

    #[test]
    fn a_loop_runs_until_it_reaches_the_instruction_that_regions_do_not_contain() {
        let (_jit, region) = counting_loop();
        assert_eq!(region.instructions, 2);
        let (registers, ctx) = run(&region, 10, 1000);
        assert_eq!(registers[10], 10);
        assert_eq!(ctx.retired, 20);
        assert_eq!(ctx.pc, BASE + 8);
    }

    #[test]
    fn a_loop_leaves_before_the_budget_could_run_out() {
        let (_jit, region) = counting_loop();
        // Entering needs a budget of one pass; the region then checks at each jump back.
        let budget = region.max_retire as i64 + 3;
        let (registers, ctx) = run(&region, 1000, budget);
        assert!(ctx.retired as i64 <= budget);
        assert_eq!(registers[10], ctx.retired / 2);
        assert_eq!(ctx.pc, BASE);
    }

    #[test]
    fn a_jump_does_not_rely_on_registers_that_other_paths_into_the_block_set_differently() {
        // The jump goes to the address in t0. Falling in through the auipc, t0 is the address of
        // the ecall at the end of the program. Branching past it, t0 is what the caller set.
        let (_jit, region) = compile(&[
            rv!(beq a1, zero, 12),
            rv!(auipc t0, 0),
            rv!(addi t1, t1, 1),
            rv!(addi t0, t0, 16),
            rv!(jalr zero, 0(t0)),
            rv!(ecall),
        ]);
        let (_, ctx) = run(&region, 1, 1000);
        assert_eq!(ctx.pc, BASE + 20);
        let (registers, ctx) = run(&region, 0, 1000);
        assert_eq!(ctx.pc, 0x5010);
        assert_eq!(registers[5], 0x5010);
    }

    #[test]
    fn a_jump_does_not_rely_on_registers_when_overlapping_instructions_lead_into_it() {
        // Like the program above, except that the way in is a different instruction sequence
        // that reaches the addi in the middle of the first one. In it a 2-byte instruction
        // overlaps the auipc and sets t0 to 5, which the jump goes to, rounded down to 4.
        let decode = |word| Instruction::decode(word).unwrap();
        let c_li_t0_5 = Instruction { length: 2, ..decode(rv!(addi t0, zero, 5)) };
        let (_jit, region) = compile_slots(&[
            (0, decode(rv!(beq a1, zero, 2))),
            (2, decode(rv!(auipc t0, 0))),
            (4, decode(rv!(addi t0, t0, 16))),
            (6, decode(rv!(jalr zero, 0(t0)))),
            (1, decode(rv!(addi t1, t1, 1))),
            (3, c_li_t0_5),
        ]);
        let (_, ctx) = run(&region, 1, 1000);
        assert_eq!(ctx.pc, BASE + 20);
        let (_, ctx) = run(&region, 0, 1000);
        assert_eq!(ctx.pc, 20);
    }
}
