//! Checks the decoder, the encoder and the assembly syntax against LLVM.
//!
//! The tests need `llvm-mc` and `llvm-objdump` with support for RISC-V. They are looked for in
//! the directory named by the `LLVM_BIN` environment variable, then on the `PATH`, then where
//! Homebrew and Debian install LLVM. Without them the tests pass without checking anything,
//! unless `LLVM_BIN` is set, in which case they fail.

use brisc_isa::{is_compressed, Format, Instruction, Opcode, Word};
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
            let llvm =
                Self { mc: directory.join("llvm-mc"), objdump: directory.join("llvm-objdump") };
            let version = Command::new(&llvm.mc).arg("--version").output().ok()?;
            let supports_riscv = String::from_utf8_lossy(&version.stdout).contains("riscv64");
            (supports_riscv && llvm.objdump.exists()).then_some(llvm)
        };

        if let Some(directory) = env::var_os("LLVM_BIN") {
            let llvm = in_directory(Path::new(&directory));
            assert!(llvm.is_some(), "LLVM_BIN does not hold llvm-mc and llvm-objdump for RISC-V");
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

    /// Returns the LLVM names of the architecture and of the instruction sets that the crate
    /// features select.
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

    /// Assembles each line of `source` to a 32-bit instruction, also where the C extension has
    /// a shorter one.
    fn assemble(&self, source: &str) -> Vec<Word> {
        self.mc(&format!(".option norvc\n{source}"), &["-show-encoding"])
            .lines()
            .filter_map(|line| line.split_once("# encoding: [")?.1.strip_suffix(']'))
            .map(|encoding| {
                let bytes = encoding
                    .split(',')
                    .map(|byte| u8::from_str_radix(byte.trim_start_matches("0x"), 16).unwrap())
                    .collect::<Vec<_>>();
                Word::from_le_bytes(bytes.try_into().expect("an instruction of four bytes"))
            })
            .collect()
    }

    /// Disassembles each of `words`, which hold an instruction of 16 or 32 bits in their low
    /// bits. Returns the mnemonic and the operands, or `None` for a word that is not an
    /// instruction.
    ///
    /// The instruction in `words[i]` is disassembled at address `4 * i`, and operands that are
    /// addresses are relative to that.
    fn disassemble(&self, name: &str, words: &[Word]) -> Vec<Option<(String, String)>> {
        // Every word gets a label, because the disassembler starts afresh at each one, and a
        // 16-bit instruction is followed by a `c.nop` to keep the addresses regular.
        let mut source = String::from(".text\n");
        for (i, word) in words.iter().enumerate() {
            if is_compressed(*word) {
                source.push_str(&format!("r{i}: .insn 2, {:#x}\n.insn 2, 1\n", word & 0xffff));
            } else {
                source.push_str(&format!("r{i}: .insn 4, {word:#x}\n"));
            }
        }
        let object = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.o"));
        self.mc(&source, &["-filetype=obj", "-o", object.to_str().unwrap()]);

        let (_, attributes) = Self::target();
        let output = Command::new(&self.objdump)
            .args(["-d", "--no-show-raw-insn", &format!("--mattr={attributes}")])
            .arg(&object)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

        // The line after the one with a label holds the instruction at the label.
        let listing = String::from_utf8(output.stdout).unwrap();
        let mut lines = listing.lines();
        let mut instructions = Vec::with_capacity(words.len());
        while let Some(line) = lines.next() {
            if line.ends_with(&format!(" <r{}>:", instructions.len())) {
                let mut fields = lines.next().unwrap().split('\t').skip(1);
                let mnemonic = fields.next().unwrap();
                let operands = fields.next().unwrap_or_default();
                instructions
                    .push((mnemonic != "<unknown>").then(|| (mnemonic.into(), operands.into())));
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

/// Returns words with the fixed bits of every opcode and arbitrary bits in the fields of its
/// operands. Most are instructions; some are not, because an operand is out of range.
fn words_of_every_opcode(random: &mut Random) -> Vec<Word> {
    let mut words = Vec::new();
    for opcode in Opcode::ALL {
        let operands = match opcode.format() {
            Format::None => 0,
            Format::R | Format::Amo => 0x07ff_8f80,
            Format::Sources => 0x01ff_8000,
            Format::Lr => 0x060f_8f80,
            Format::I | Format::Load | Format::Csr | Format::CsrI | Format::S | Format::B => {
                0xffff_8f80
            }
            Format::U | Format::J => 0xffff_ff80,
        };
        // Shift amounts are a small part of the immediate, so the bits above them are only
        // rarely set.
        let shift = matches!(opcode.mnemonic().as_bytes(), [b's', b'l' | b'r', _, b'i', ..]);
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
    // The fields of a fence that have no meaning yet are to be ignored, which is what decoding
    // them as operands amounts to. LLVM only knows fences in which they are zero.
    if let Some(instruction) = decoded {
        return matches!(instruction.opcode, Opcode::Fence | Opcode::FenceI);
    }
    // LLVM reads the all-zero halfword, which is defined to be illegal, as `c.unimp`, and knows
    // the return from debug mode.
    word & 0xffff == 0 || word == 0x7b20_0073
}

/// LLVM and [Instruction::decode] accept the same words as instructions.
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
            assert!(known_disagreement(word, decoded), "{word:#010x}: {decoded:?}, llvm: {llvm:?}");
        }
    }
}

/// LLVM assembles the text that [Instruction] displays as to the word that
/// [Instruction::encode] returns.
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
        let expected = opcode.is_supported() && !matches!(opcode, Opcode::Fence | Opcode::FenceI);
        assert_eq!(covered, expected, "{opcode}");
    }

    let source =
        instructions.iter().map(|instruction| format!("{instruction}\n")).collect::<String>();
    let assembled = llvm.assemble(&source);
    assert_eq!(assembled.len(), instructions.len());
    for (instruction, assembled) in instructions.into_iter().zip(assembled) {
        assert_eq!(instruction.encode(), Some(assembled), "{instruction}");
    }
}

/// A 16-bit instruction decodes to the instruction that LLVM says it abbreviates.
///
/// LLVM disassembles a 16-bit instruction as the 32-bit instruction it stands for. Assembling
/// that text without the C extension gives a 32-bit word, which must decode to the same
/// operands as the 16-bit instruction.
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
        let (mnemonic, operands) = llvm.expect("checked by decoding_accepts_what_llvm_accepts");
        // LLVM does not expand hints, which only write `x0`.
        if mnemonic.starts_with("c.") {
            continue;
        }
        // An operand that is an address is followed by the label it is relative to. The
        // assembler wants the offset from the instruction instead.
        let operands = match operands.split_once(" <") {
            Some((operands, _)) => {
                let (registers, address) = operands.rsplit_once(' ').unwrap_or(("", operands));
                let address = u64::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
                let offset = address.wrapping_sub(4 * i as u64);
                // Addresses wrap around at the width of a register.
                let offset =
                    if cfg!(feature = "64-bit") { offset as i64 } else { offset as i32 as i64 };
                format!("{registers} {offset}")
            }
            None => operands,
        };
        // LLVM writes `add rd, zero, rs2`, the expansion of `c.mv`, as `mv rd, rs2`, which the
        // assembler takes to mean `addi rd, rs2, 0`.
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
