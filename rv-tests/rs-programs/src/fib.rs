#![no_std]
#![no_main]

use syscalls::Sysno;

fn fib(n: u64) -> u64 {
    if n == 0 {
        return 0;
    } else if n == 1 {
        return 1;
    } else {
        return fib(n - 1) + fib(n - 2);
    }
}

#[no_mangle]
pub extern "C" fn _start() {
    let _n = fib(30);
    unsafe { let _ = syscalls::syscall1(Sysno::exit, 0); }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { let _ = syscalls::syscall1(Sysno::exit, 2); }
    panic!();
}
