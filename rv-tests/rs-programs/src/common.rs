//! Entry points shared by the guest programs that have no dependencies.
//!
//! Each program is a function that returns a checksum. On a RISC-V target the checksum is compared
//! with the value that the program produces when it runs natively on the host and the exit code
//! says whether they match, so a program doubles as a correctness test of the emulator. On any
//! other target the program prints its checksum, which is how the expected value is obtained.

/// Exits the program with `code`.
#[cfg(target_os = "none")]
pub fn exit(code: usize) -> ! {
    // SAFETY: The program ends here and the host handles the system call.
    unsafe { core::arch::asm!("ecall", in("a7") 93usize, in("a0") code, options(noreturn)) }
}

/// Defines the entry point of a program: `$kernel` is run and its checksum compared with
/// `$expected`.
macro_rules! entry {
    ($kernel:path, $expected:expr) => {
        #[cfg(target_os = "none")]
        #[no_mangle]
        pub extern "C" fn _start() -> ! {
            let checksum: u64 = $kernel();
            $crate::common::exit(if checksum == $expected { 0 } else { 1 })
        }

        #[cfg(target_os = "none")]
        #[panic_handler]
        fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
            $crate::common::exit(2)
        }

        #[cfg(not(target_os = "none"))]
        fn main() {
            println!("{}", $kernel());
        }
    };
}

/// A small deterministic generator for the input of the programs.
pub struct XorShift(pub u64);

impl XorShift {
    /// Returns the next value.
    #[inline(always)]
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

