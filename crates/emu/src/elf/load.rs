//! ELF file loading utilities.

use crate::st::StEmu;
use alloc::{
    format,
    string::{String, ToString},
};
use brisc_hw::{
    linux::SyscallInterface,
    memory::{Address, Memory},
    XWord,
};
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};

/// Load a raw ELF file into a [StEmu] object.
///
/// ### Takes
/// - `raw`: The raw contents of the ELF file to load.
///
/// ### Returns
/// - `Ok(state)` if the ELF file was loaded successfully
/// - `Err(_)` if the ELF file could not be loaded
pub fn load_elf<M, S>(raw: &[u8]) -> Result<StEmu<M, S>, String>
where
    M: Memory + Clone + Default,
    S: SyscallInterface + Default,
{
    let elf = ElfBytes::<AnyEndian>::minimal_parse(raw)
        .map_err(|e| format!("Failed to parse ELF file: {e}"))?;
    let mut memory = M::default();

    let headers = elf.segments().ok_or("Failed to load section headers")?;
    for (i, header) in headers.iter().enumerate() {
        if header.p_type == 0x70000003 {
            continue;
        }

        let segment_data =
            elf.segment_data(&header).map_err(|e| format!("Failed to fetch section data: {e}"))?;
        let section_data = &segment_data[..header.p_filesz as usize];
        let mut data = section_data.to_vec();

        if header.p_filesz != header.p_memsz {
            if header.p_type == PT_LOAD {
                if header.p_filesz < header.p_memsz {
                    data.resize(data.len() + (header.p_memsz - header.p_filesz) as usize, 0);
                } else {
                    return Err(format!(
                        "Invalid PT_LOAD program segment {}, file size ({}) > mem size ({})",
                        i, header.p_filesz, header.p_memsz
                    ));
                }
            } else {
                return Err(format!(
                    "Program segment {} has different file size ({}) than mem size ({}): filling for non PT_LOAD segments is not supported",
                    i,
                    header.p_filesz,
                    header.p_memsz
                ));
            }
        }

        if header.p_vaddr + header.p_memsz >= 1 << 47 {
            return Err(format!(
                "Program segment {} out of 64-bit mem range: {} - {} (size: {})",
                i,
                header.p_vaddr,
                header.p_vaddr + header.p_memsz,
                header.p_memsz
            ));
        }

        memory
            .set_memory_range(header.p_vaddr as Address, &mut data.as_slice())
            .map_err(|e| e.to_string())?;
    }

    Ok(StEmu::new(elf.ehdr.e_entry as XWord, memory, S::default()))
}
