#![no_std]
#![no_main]

use alloc::{vec, vec::Vec};
use linked_list_allocator::LockedHeap;
use syscalls::Sysno;

extern crate alloc;

/// The global allocator for the program in other profiles uses the [SpinLockedAllocator].
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe fn init_allocator(heap_start_addr: *mut u8, heap_size: usize) {
    ALLOCATOR.lock().init(heap_start_addr, heap_size)
}

#[no_mangle]
pub extern "C" fn _start() {
    // Initialize a 20MB heap.
    static mut HEAP: [u8; 20_000_000] = [0u8; 20_000_000];
    unsafe { init_allocator(HEAP.as_mut_ptr(), 20_000_000) }

    const RAND_SEED: u64 = 0x123456789abcdef0;
    let mut contents = vec![0xff; 256_000]
        .into_iter()
        .enumerate()
        .map(|x| {
            let (i, _) = x;
            ((i as u64).wrapping_mul(RAND_SEED)) % 0xff
        })
        .collect::<Vec<u64>>();
    let _sorted = contents.sort();

    unsafe {
        syscalls::syscall1(Sysno::exit, 0);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        syscalls::syscall1(Sysno::exit, 2);
    }
    panic!();
}
