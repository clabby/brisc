//! ELF file loading utilities.

use alloc::{format, string::String};
use brisc_hw::{
    memory::{Address, Memory},
    XWord,
};
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};
use thiserror::Error;

/// The reason an ELF file was not loaded.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum LoadError<M> {
    /// The file is not an ELF file that can be loaded. The message says why.
    #[error("{0}")]
    Elf(String),
    /// The memory failed the write of a segment.
    #[error("the memory failed the write of a segment")]
    Memory(M),
}

/// Loads a raw ELF file onto a [`Memory`].
///
/// ### Takes
/// - `memory`: The memory that the segments of the file are written to.
/// - `raw`: The raw contents of the ELF file to load.
///
/// ### Returns
/// - `Ok(entry_pc)` if the ELF file was loaded successfully
/// - `Err(_)` if the ELF file could not be loaded
pub fn load_elf<M: Memory>(memory: &mut M, raw: &[u8]) -> Result<XWord, LoadError<M::Error>> {
    let elf = ElfBytes::<AnyEndian>::minimal_parse(raw)
        .map_err(|e| LoadError::Elf(format!("Failed to parse ELF file: {e}")))?;

    let headers =
        elf.segments().ok_or_else(|| LoadError::Elf("Failed to load section headers".into()))?;
    for (i, header) in headers.iter().enumerate() {
        if header.p_type == 0x70000003 {
            continue;
        }

        let segment_data = elf
            .segment_data(&header)
            .map_err(|e| LoadError::Elf(format!("Failed to fetch section data: {e}")))?;
        if header.p_filesz != header.p_memsz {
            if header.p_type == PT_LOAD {
                if header.p_filesz > header.p_memsz {
                    return Err(LoadError::Elf(format!(
                        "Invalid PT_LOAD program segment {}, file size ({}) > mem size ({})",
                        i, header.p_filesz, header.p_memsz
                    )));
                }
            } else {
                return Err(LoadError::Elf(format!(
                    "Program segment {} has different file size ({}) than mem size ({}): filling for non PT_LOAD segments is not supported",
                    i,
                    header.p_filesz,
                    header.p_memsz
                )));
            }
        }

        if header.p_vaddr + header.p_memsz >= 1 << 47 {
            return Err(LoadError::Elf(format!(
                "Program segment {} out of 64-bit mem range: {} - {} (size: {})",
                i,
                header.p_vaddr,
                header.p_vaddr + header.p_memsz,
                header.p_memsz
            )));
        }

        // Only the bytes present in the file are written. The rest of the segment is expected
        // to read as zero already.
        memory
            .write(header.p_vaddr as Address, &segment_data[..header.p_filesz as usize])
            .map_err(LoadError::Memory)?;
    }

    Ok(elf.ehdr.e_entry as XWord)
}
